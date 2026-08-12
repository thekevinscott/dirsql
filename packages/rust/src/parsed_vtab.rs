//! A path-table whose columns come from a parser command's row objects.
//!
//! `CREATE VIRTUAL TABLE t USING dirsql_parsed('<root>', '<glob>', '<parser>')`
//!
//! Unlike the stat path-table ([`crate::vtab`]), whose seven columns are known
//! in advance, this table's columns are whatever the parser emitted. A vtab
//! must declare its schema before any row can flow, so the work happens at
//! `CREATE`: every matched file is parsed, the rows are inferred over
//! ([`crate::infer`]), the schema is declared, and those same rows are then
//! served. The sample and the result are the same rows, so the declared schema
//! always describes the data exactly.
//!
//! The trade-off against the stat path-table is deliberate: reads there are
//! live because the scan happens per statement, whereas here the rows are
//! materialized once. Re-parsing every file on every statement would make a
//! join over a parsed table quadratic in parser invocations, and re-inferring
//! could change the schema out from under a prepared statement.
//!
//! Under `--persist` that materialization is cached across *runs* too: a file
//! whose stat tuple has not moved serves the payload the parser produced last
//! time and the process is never spawned. See [`crate::parsed_cache`].

use std::collections::{HashMap, HashSet};
use std::ffi::c_int;
use std::path::{Path, PathBuf};

use globset::GlobSet;
use rusqlite::vtab::{
    Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind, Values,
    read_only_module,
};
use rusqlite::{Connection, Error, Result, ffi};

use crate::Value;
use crate::command::{Placeholder, run_command};
use crate::infer::{JsonRow, cell, declared_schema, infer_schema, parse_rows};
use crate::matcher::TableMatcher;
use crate::parsed_cache::{self, CachedParse, Entry, RowCache, SqliteRowCache};
use crate::path_table;
use crate::persist::{FileStat, hash_file, now_ns};
use crate::scanner::{self, scan_glob};
use crate::sql_literal::unquote;

/// SQL module name a parsed path-table is created with.
pub const MODULE_NAME: &str = "dirsql_parsed";

/// Register the parsed path-table module on `conn`.
pub fn load_module(conn: &Connection) -> Result<()> {
    let aux: Option<()> = None;
    conn.create_module(MODULE_NAME, read_only_module::<ParsedTab>(), aux)
}

/// Number of module arguments that are not ignore patterns.
const FIXED_ARGS: usize = 5;

/// The module's own arguments: root, glob pattern, parser command, the
/// gitignore switch, the persistent cache path (empty when the index is
/// ephemeral), and the skip rules the scan applies (mirroring the stat
/// path-table's ignore args).
struct ModuleArgs {
    root: PathBuf,
    pattern: String,
    glob: GlobSet,
    command: String,
    gitignore: bool,
    cache: Option<PathBuf>,
    ignore: TableMatcher,
}

/// Compile the ignore patterns a parsed path-table scan applies.
fn compile_ignore(patterns: &[String]) -> Result<TableMatcher> {
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    TableMatcher::new(&[], &refs).map_err(|e| Error::ModuleError(e.to_string()))
}

/// Parse a parsed path-table's `CREATE VIRTUAL TABLE` arguments. `args[0..3]`
/// are the module, database and table names; the module's own follow — root,
/// glob, parser, the gitignore switch, the cache path, then any ignore
/// patterns.
fn parse_module_args(args: &[&[u8]]) -> Result<ModuleArgs> {
    let user_args: Vec<String> = args
        .iter()
        .skip(3)
        .map(|a| unquote(&String::from_utf8_lossy(a)).to_string())
        .collect();

    let [root, pattern, command, gitignore, cache, ignore @ ..] = user_args.as_slice() else {
        return Err(Error::ModuleError(format!(
            "{MODULE_NAME} takes at least {FIXED_ARGS} arguments \
             (root, glob, parser, gitignore switch, cache path), got {}",
            user_args.len()
        )));
    };

    let glob = scanner::compile_glob(pattern).map_err(|e| Error::ModuleError(e.to_string()))?;

    Ok(ModuleArgs {
        root: PathBuf::from(root),
        pattern: pattern.clone(),
        glob,
        command: command.clone(),
        gitignore: scanner::parse_gitignore_arg(gitignore).map_err(Error::ModuleError)?,
        // The empty string is how "no cache" is spelled: a module argument
        // cannot be absent without shifting every argument after it.
        cache: Some(PathBuf::from(cache)).filter(|p| !p.as_os_str().is_empty()),
        ignore: compile_ignore(ignore)?,
    })
}

/// Run the parser over every matched file and concatenate the rows.
///
/// `run` is injected so the fan-out and the ordering can be unit-tested without
/// spawning a process; `warn` is injected so the skip warnings can be captured.
/// Production passes a closure over [`run_parser`] and `|m| eprintln!("{m}")`.
///
/// Per-file isolation, matching the `on-file` hook contract: a file whose
/// parser fails (spawn/exit/timeout/no-output) or whose output does not parse
/// contributes no rows and a one-line warning to `warn`; the scan continues.
/// The schema is inferred from whatever files did parse.
fn collect_rows(
    rel_paths: &[PathBuf],
    run: &dyn Fn(&Path) -> std::result::Result<String, String>,
    warn: &dyn Fn(&str),
) -> Vec<JsonRow> {
    let mut all = Vec::new();
    for rel_path in rel_paths {
        match run(rel_path) {
            Err(error) => warn(&command_skip_message(rel_path, &error)),
            Ok(payload) => match parse_rows(&payload) {
                Ok(rows) => all.extend(rows),
                Err(message) => warn(&parse_skip_message(rel_path, &message)),
            },
        }
    }
    all
}

/// The rows for one file, and whether they came from a fresh parse that the
/// cache should learn.
struct Parsed {
    payload: String,
    stat: FileStat,
    fresh: bool,
}

/// The same fan-out as [`collect_rows`], with the persistent cache in front of
/// the parser: a file whose stat tuple matches its cached entry serves that
/// entry's payload and is never handed to `run`.
///
/// Everything effectful is injected — `stat`, `hash`, `run`, `warn` — so the
/// reuse decision can be exercised without a filesystem or a child process.
/// `hash` is only ever called for a file the stat tuple alone cannot settle,
/// which is what keeps an unchanged tree free of file reads.
///
/// The cache is only written when the scan actually changed something, which is
/// what lets an unchanged tree leave the cache file byte-for-byte alone. A file
/// whose parse failed is left uncached and retried next run, matching the
/// declared-table contract: the cache is incomplete, never wrong.
///
/// A failed *write* is warned about and swallowed. The rows are already correct
/// and the next run simply re-parses; failing the user's query over a lost
/// optimization would be the worse outcome. It is reachable: the cache file is
/// normally WAL, where this connection's write and the owning connection's read
/// coexist, but WAL is unavailable on some filesystems and there the two can
/// genuinely contend.
fn collect_rows_cached(
    cache: &dyn RowCache,
    rel_paths: &[PathBuf],
    fs: &dyn ParsedFs,
    run: &dyn Fn(&Path) -> std::result::Result<String, String>,
    warn: &dyn Fn(&str),
) -> Result<Vec<JsonRow>> {
    let cached: HashMap<String, CachedParse> = cache.read()?;
    let snapshot_ns = now_ns();

    let mut seen: Vec<(String, Parsed)> = Vec::with_capacity(rel_paths.len());
    for rel_path in rel_paths {
        let key = rel_path.to_string_lossy().into_owned();
        let Some(live) = fs.stat(rel_path) else {
            // The file vanished between the scan and the stat. Nothing to
            // parse and nothing to cache; the next run decides afresh.
            continue;
        };
        let entry = cached.get(&key);
        if parsed_cache::is_fresh(entry, &live, || fs.hash(rel_path)) {
            let payload = entry.expect("is_fresh implies an entry").payload.clone();
            seen.push((
                key,
                Parsed {
                    payload,
                    stat: live,
                    fresh: false,
                },
            ));
            continue;
        }
        match run(rel_path) {
            Err(error) => warn(&command_skip_message(rel_path, &error)),
            Ok(payload) => seen.push((
                key,
                Parsed {
                    payload,
                    stat: live,
                    fresh: true,
                },
            )),
        }
    }

    let mut rows = Vec::new();
    let mut writes: Vec<Entry<'_>> = Vec::new();
    for (key, parsed) in &seen {
        match parse_rows(&parsed.payload) {
            Ok(parsed_rows) => {
                rows.extend(parsed_rows);
                if parsed.fresh {
                    writes.push(Entry {
                        rel_path: key,
                        stat: &parsed.stat,
                        content_hash: fs.hash(Path::new(key)),
                        snapshot_ns,
                        payload: &parsed.payload,
                    });
                }
            }
            Err(message) => warn(&parse_skip_message(Path::new(key), &message)),
        }
    }

    let live: HashSet<&str> = seen.iter().map(|(key, _)| key.as_str()).collect();
    let stale: Vec<&str> = cached
        .keys()
        .map(String::as_str)
        .filter(|key| !live.contains(key))
        .collect();

    if (!writes.is_empty() || !stale.is_empty())
        && let Err(error) = cache.commit(&writes, &stale)
    {
        warn(&cache_write_skip_message(&error));
    }

    Ok(rows)
}

/// Warning for a cache the run could not update. Says what was lost (the reuse,
/// not the rows) so the reader knows this is a slow next run, not a wrong one.
fn cache_write_skip_message(error: &Error) -> String {
    format!(
        "dirsql: could not update the persist cache: {error}; rows are unaffected, the next run re-parses"
    )
}

/// The filesystem questions the cached collection asks, injected so the reuse
/// decision is testable without a real tree.
trait ParsedFs {
    /// The file's stat tuple, or `None` when it is gone.
    fn stat(&self, rel_path: &Path) -> Option<FileStat>;
    /// The file's content hash. Best-effort: `None` only costs a re-parse.
    fn hash(&self, rel_path: &Path) -> Option<[u8; 32]>;
}

/// The production [`ParsedFs`]: paths resolved against the scan root.
struct RootedFs<'a> {
    root: &'a Path,
}

impl ParsedFs for RootedFs<'_> {
    fn stat(&self, rel_path: &Path) -> Option<FileStat> {
        std::fs::metadata(self.root.join(rel_path))
            .ok()
            .map(|meta| FileStat::from_metadata(&meta))
    }

    fn hash(&self, rel_path: &Path) -> Option<[u8; 32]> {
        hash_file(&self.root.join(rel_path)).ok()
    }
}

/// Warning for a file whose parser command itself failed. Mirrors the `on-file`
/// hook's wording so both surfaces read identically.
fn command_skip_message(rel_path: &Path, error: &str) -> String {
    format!(
        "dirsql: skipping `{}`: on-file command failed: {error}",
        rel_path.display()
    )
}

/// Warning for a file whose parser output was not a JSON array of rows.
fn parse_skip_message(rel_path: &Path, message: &str) -> String {
    format!(
        "dirsql: skipping `{}`: on-file output was not a JSON array of rows: {message}",
        rel_path.display()
    )
}

/// The error raised when the sample yields nothing to infer from. SQLite has
/// no zero-column table, and inventing a placeholder column would make
/// `SELECT *` mean something the parser never said.
fn no_rows_message(pattern: &str) -> String {
    format!("{MODULE_NAME}: parser produced no rows for `{pattern}`; cannot infer a schema")
}

/// Run the parser for one file. `{path}` is the file's absolute path and
/// `{root}` the scan root, matching the `on-file` contract.
fn run_parser(command: &str, root: &Path, rel_path: &Path) -> std::result::Result<String, String> {
    let abs_path = root.join(rel_path);
    let placeholders = [
        Placeholder::new("path", abs_path.to_string_lossy().into_owned()),
        Placeholder::new("root", root.to_string_lossy().into_owned()),
    ];

    run_command(command, &placeholders, root, None)
        .map(|output| output.payload)
        .map_err(|e| e.to_string())
}

/// Column name for an index, or `None` when SQLite asks for one out of range.
fn column_name(names: &[String], i: c_int) -> Option<&str> {
    let index = usize::try_from(i).ok()?;
    names.get(index).map(String::as_str)
}

/// Whether the cursor has run past the last row.
fn at_eof(index: usize, row_count: usize) -> bool {
    index >= row_count
}

/// Rowid for a cursor position, saturating rather than wrapping on a row count
/// no filesystem will produce.
fn rowid_of(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

#[repr(C)]
struct ParsedTab {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
    /// Only the names survive registration: the types live in the schema
    /// SQLite already holds, and a cursor addresses columns by index.
    column_names: Vec<String>,
    rows: Vec<JsonRow>,
}

#[expect(unsafe_code, reason = "rusqlite requires VTab to be an unsafe trait")]
unsafe impl<'vtab> VTab<'vtab> for ParsedTab {
    type Aux = ();
    type Cursor = ParsedTabCursor;

    fn connect(
        _db: &mut VTabConnection,
        _aux: Option<&()>,
        args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let ModuleArgs {
            root,
            pattern,
            glob,
            command,
            gitignore,
            cache,
            ignore,
        } = parse_module_args(args)?;

        // A parsed path-table honors the same skip rules a stat path-table does
        // (node_modules/.git, gitignore, plus any configured ignore), so a
        // parsed `SELECT * FROM './'` doesn't drown in dependency trees.
        let ignore_base = path_table::ignore_base(&pattern);
        let rel_paths = scan_glob(&root, &glob, &ignore, &ignore_base, gitignore);
        let run = |rel: &Path| run_parser(&command, &root, rel);
        let warn = |message: &str| eprintln!("{message}");
        let rows = match &cache {
            None => collect_rows(&rel_paths, &run, &warn),
            Some(path) => {
                let key = parsed_cache::table_key(&root, &pattern, &command);
                let cache = SqliteRowCache::open(path, key)?;
                collect_rows_cached(&cache, &rel_paths, &RootedFs { root: &root }, &run, &warn)?
            }
        };

        let columns = infer_schema(&rows);
        if columns.is_empty() {
            return Err(Error::ModuleError(no_rows_message(&pattern)));
        }

        let schema = declared_schema(&columns);
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
            column_names: columns.into_iter().map(|c| c.name).collect(),
            rows,
        };
        Ok((schema, vtab))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        info.set_estimated_cost(1000.);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<ParsedTabCursor> {
        Ok(ParsedTabCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            column_names: self.column_names.clone(),
            rows: self.rows.clone(),
            index: 0,
        })
    }
}

impl<'vtab> CreateVTab<'vtab> for ParsedTab {
    const KIND: VTabKind = VTabKind::Default;
}

#[repr(C)]
struct ParsedTabCursor {
    /// Base class. Must be first: `rust_open` hands this pointer straight to
    /// SQLite as a `sqlite3_vtab_cursor`, so anything ahead of it gets
    /// overwritten.
    base: ffi::sqlite3_vtab_cursor,
    column_names: Vec<String>,
    rows: Vec<JsonRow>,
    index: usize,
}

#[expect(
    unsafe_code,
    reason = "rusqlite requires VTabCursor to be an unsafe trait"
)]
unsafe impl VTabCursor for ParsedTabCursor {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _args: &Values<'_>,
    ) -> Result<()> {
        self.index = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.index += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        at_eof(self.index, self.rows.len())
    }

    fn column(&self, ctx: &mut Context, i: c_int) -> Result<()> {
        let value = self
            .rows
            .get(self.index)
            .zip(column_name(&self.column_names, i))
            .map(|(row, name)| cell(row, name))
            .unwrap_or(Value::Null);
        ctx.set_result(&value)
    }

    fn rowid(&self) -> Result<i64> {
        Ok(rowid_of(self.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vtab behavior over a real directory and a real parser process is
    // covered by `tests/schema_inference.rs` (unit-lint isolation); only the
    // pure helpers are tested here.

    #[test]
    fn module_name_is_stable() {
        assert_eq!(MODULE_NAME, "dirsql_parsed");
    }

    /// The three fixed arguments SQLite prepends before the module's own.
    fn args_with<'a>(user: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut all: Vec<&'a [u8]> = vec![
            b"dirsql_parsed".as_slice(),
            b"main".as_slice(),
            b"t".as_slice(),
        ];
        all.extend_from_slice(user);
        all
    }

    #[test]
    fn parse_module_args_extracts_root_glob_and_command() {
        let args = args_with(&[
            b"'/tmp/notes'",
            b"'**/*.json'",
            b"'cat {path}'",
            b"'gitignore'",
            b"''",
        ]);
        let parsed = parse_module_args(&args).unwrap();

        assert_eq!(parsed.root, PathBuf::from("/tmp/notes"));
        assert_eq!(parsed.pattern, "**/*.json");
        assert_eq!(parsed.command, "cat {path}");
        assert!(parsed.glob.is_match(Path::new("a.json")));
        assert!(!parsed.glob.is_match(Path::new("a.md")));
    }

    #[test]
    fn parse_module_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes", b"**/*", b"cat", b"gitignore", b""]);
        assert_eq!(
            parse_module_args(&args).unwrap().root,
            PathBuf::from("/tmp/notes")
        );
    }

    #[test]
    fn parse_module_args_reads_the_gitignore_switch() {
        let on = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"''"]);
        assert!(parse_module_args(&on).unwrap().gitignore);

        let off = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'no-gitignore'", b"''"]);
        assert!(!parse_module_args(&off).unwrap().gitignore);
    }

    #[test]
    fn parse_module_args_rejects_an_unknown_gitignore_switch() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'sometimes'", b"''"]);
        let err = match parse_module_args(&args) {
            Err(err) => err,
            Ok(_) => panic!("an unknown switch must be rejected"),
        };
        assert!(err.to_string().contains("no-gitignore"), "got: {err}");
    }

    #[test]
    fn parse_module_args_compiles_trailing_ignore_patterns() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'node_modules/**'",
        ]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
        assert!(!parsed.ignore.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn parse_module_args_with_no_ignore_patterns_ignores_nothing() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"''"]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(!parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
    }

    #[test]
    fn parse_module_args_rejects_too_few_arguments() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'"]);
        let err = match parse_module_args(&args) {
            Err(err) => err,
            Ok(_) => panic!("four arguments must be rejected"),
        };
        assert!(
            err.to_string().contains("at least"),
            "error should name the arity, got: {err}"
        );
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_glob() {
        let args = args_with(&[b"'/tmp'", b"'['", b"'cat'", b"'gitignore'", b"''"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn parse_module_args_reads_the_cache_path() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"'/cache/dirsql.db'",
        ]);
        assert_eq!(
            parse_module_args(&args).unwrap().cache,
            Some(PathBuf::from("/cache/dirsql.db"))
        );
    }

    #[test]
    fn parse_module_args_reads_an_empty_cache_path_as_no_cache() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"''"]);
        assert_eq!(parse_module_args(&args).unwrap().cache, None);
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_ignore_pattern() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'['",
        ]);
        assert!(parse_module_args(&args).is_err());
    }

    fn ok(payload: &'static str) -> impl Fn(&Path) -> std::result::Result<String, String> {
        move |_| Ok(payload.to_string())
    }

    fn collect_with_warnings(
        paths: &[PathBuf],
        run: &dyn Fn(&Path) -> std::result::Result<String, String>,
    ) -> (Vec<JsonRow>, Vec<String>) {
        let warnings = std::cell::RefCell::new(Vec::new());
        let rows = collect_rows(paths, run, &|m| warnings.borrow_mut().push(m.to_string()));
        (rows, warnings.into_inner())
    }

    #[test]
    fn collect_rows_concatenates_every_files_rows_in_scan_order() {
        let paths = vec![PathBuf::from("a.json"), PathBuf::from("b.json")];
        let (rows, warnings) = collect_with_warnings(&paths, &|rel| {
            Ok(format!(r#"[{{"id":"{}"}}]"#, rel.display()))
        });

        let ids: Vec<_> = rows
            .iter()
            .map(|r| r.get("id").unwrap().as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, vec!["a.json", "b.json"]);
        assert!(
            warnings.is_empty(),
            "no failures, no warnings: {warnings:?}"
        );
    }

    #[test]
    fn collect_rows_keeps_every_row_a_file_emitted() {
        let paths = vec![PathBuf::from("a.json")];
        let (rows, _) = collect_with_warnings(&paths, &ok(r#"[{"i":1},{"i":2}]"#));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn collect_rows_over_no_files_is_no_rows() {
        let (rows, warnings) = collect_with_warnings(&[], &ok("[]"));
        assert_eq!(rows, Vec::new());
        assert!(warnings.is_empty());
    }

    #[test]
    fn collect_rows_skips_a_run_failure_warns_and_continues() {
        let paths = vec![PathBuf::from("bad.json"), PathBuf::from("good.json")];
        let (rows, warnings) = collect_with_warnings(&paths, &|rel| {
            if rel == Path::new("bad.json") {
                Err("exit 7".to_string())
            } else {
                Ok(r#"[{"i":1}]"#.to_string())
            }
        });

        assert_eq!(rows.len(), 1, "the good file still contributes its row");
        assert_eq!(warnings.len(), 1, "the bad file warns once: {warnings:?}");
        assert!(warnings[0].contains("bad.json"), "got: {}", warnings[0]);
        assert!(warnings[0].contains("exit 7"), "got: {}", warnings[0]);
    }

    #[test]
    fn collect_rows_skips_a_parse_failure_warns_and_continues() {
        let paths = vec![PathBuf::from("bad.json"), PathBuf::from("good.json")];
        let (rows, warnings) = collect_with_warnings(&paths, &|rel| {
            if rel == Path::new("bad.json") {
                Ok("not json".to_string())
            } else {
                Ok(r#"[{"i":1}]"#.to_string())
            }
        });

        assert_eq!(rows.len(), 1);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bad.json"), "got: {}", warnings[0]);
    }

    #[test]
    fn command_skip_message_names_the_file_and_the_error() {
        let message = command_skip_message(Path::new("a/b.json"), "exit 3");
        assert!(message.contains("a/b.json"), "got: {message}");
        assert!(message.contains("exit 3"), "got: {message}");
        assert!(message.contains("on-file command failed"), "got: {message}");
    }

    #[test]
    fn parse_skip_message_names_the_file_and_the_defect() {
        let message = parse_skip_message(Path::new("a/b.json"), "expected an array");
        assert!(message.contains("a/b.json"), "got: {message}");
        assert!(
            message.contains("not a JSON array of rows"),
            "got: {message}"
        );
    }

    #[test]
    fn no_rows_message_names_the_glob_and_says_no_rows() {
        let message = no_rows_message("**/*.json");
        assert!(message.contains("no rows"), "got: {message}");
        assert!(message.contains("**/*.json"), "got: {message}");
    }

    fn names() -> Vec<String> {
        vec!["a".to_string(), "b".to_string()]
    }

    #[test]
    fn column_name_resolves_a_declared_index() {
        assert_eq!(column_name(&names(), 0), Some("a"));
        assert_eq!(column_name(&names(), 1), Some("b"));
    }

    #[test]
    fn column_name_is_none_past_the_last_column() {
        assert_eq!(column_name(&names(), 2), None);
    }

    #[test]
    fn column_name_is_none_for_a_negative_index() {
        assert_eq!(column_name(&names(), -1), None);
    }

    #[test]
    fn at_eof_is_false_while_rows_remain() {
        assert!(!at_eof(0, 2));
        assert!(!at_eof(1, 2));
    }

    #[test]
    fn at_eof_is_true_once_past_the_last_row() {
        assert!(at_eof(2, 2));
        assert!(at_eof(3, 2));
    }

    #[test]
    fn at_eof_is_true_for_an_empty_row_set() {
        assert!(at_eof(0, 0));
    }

    #[test]
    fn rowid_of_tracks_the_cursor_position() {
        assert_eq!(rowid_of(0), 0);
        assert_eq!(rowid_of(7), 7);
    }

    #[test]
    fn rowid_of_saturates_rather_than_wrapping() {
        assert_eq!(rowid_of(usize::MAX), i64::MAX);
    }

    // The run_parser tests spawn a real `sh`. Their test code statically
    // references only `super::` items (plus pure std), matching the pattern
    // `command.rs` uses for the runner underneath.

    #[test]
    fn run_parser_returns_the_commands_payload() {
        let payload = run_parser(
            "sh -c 'echo chatter; echo PAYLOAD'",
            Path::new("."),
            Path::new("a.json"),
        )
        .unwrap();
        assert_eq!(payload, "PAYLOAD");
    }

    #[test]
    fn run_parser_substitutes_the_absolute_path_and_the_root() {
        let payload =
            run_parser("echo {path} {root}", Path::new("/tmp"), Path::new("a.json")).unwrap();
        assert_eq!(payload, "/tmp/a.json /tmp");
    }

    #[test]
    fn run_parser_surfaces_a_command_failure_as_a_message() {
        let err = run_parser("sh -c 'exit 7'", Path::new("."), Path::new("a.json")).unwrap_err();
        assert!(err.contains('7'), "the exit code is reported, got: {err}");
    }

    /// A [`ParsedFs`] over a fixed table of stats and hashes, so the reuse
    /// decision is exercised without a filesystem. `hashed` records every hash
    /// asked for, which is how "an unchanged tree reads no files" is asserted.
    #[derive(Default)]
    struct FakeFs {
        stats: HashMap<String, FileStat>,
        hashes: HashMap<String, [u8; 32]>,
        hashed: std::cell::RefCell<Vec<String>>,
    }

    impl FakeFs {
        fn with(paths: &[(&str, i64)]) -> Self {
            let mut fs = Self::default();
            for (path, mtime_ns) in paths {
                fs.stats.insert((*path).to_string(), stat(*mtime_ns));
            }
            fs
        }
    }

    impl ParsedFs for FakeFs {
        fn stat(&self, rel_path: &Path) -> Option<FileStat> {
            self.stats
                .get(&rel_path.to_string_lossy().to_string())
                .cloned()
        }

        fn hash(&self, rel_path: &Path) -> Option<[u8; 32]> {
            let key = rel_path.to_string_lossy().to_string();
            self.hashed.borrow_mut().push(key.clone());
            self.hashes.get(&key).copied()
        }
    }

    /// An in-memory [`RowCache`] that records what it was asked to commit, so
    /// the unit under test is exercised without a database.
    #[derive(Default)]
    struct FakeCache {
        entries: std::cell::RefCell<HashMap<String, CachedParse>>,
        commits: std::cell::RefCell<usize>,
        fail_read: bool,
        fail_commit: bool,
    }

    impl FakeCache {
        /// Seed the cache as a prior run would have, with a snapshot far enough
        /// ahead of the file's mtime to be outside the racy window.
        fn seeded(entries: &[(&str, i64, &str)]) -> Self {
            let cache = Self::default();
            for (rel_path, mtime_ns, payload) in entries {
                cache.put(rel_path, stat(*mtime_ns), None, mtime_ns + 1, payload);
            }
            cache
        }

        fn put(
            &self,
            rel_path: &str,
            stat: FileStat,
            content_hash: Option<[u8; 32]>,
            snapshot_ns: i64,
            payload: &str,
        ) {
            self.entries.borrow_mut().insert(
                rel_path.to_string(),
                CachedParse {
                    rel_path: rel_path.to_string(),
                    stat,
                    content_hash,
                    snapshot_ns,
                    payload: payload.to_string(),
                },
            );
        }

        fn payloads(&self) -> std::collections::BTreeMap<String, String> {
            self.entries
                .borrow()
                .iter()
                .map(|(k, v)| (k.clone(), v.payload.clone()))
                .collect()
        }
    }

    impl RowCache for FakeCache {
        fn read(&self) -> rusqlite::Result<HashMap<String, CachedParse>> {
            if self.fail_read {
                return Err(rusqlite::Error::InvalidQuery);
            }
            Ok(self.entries.borrow().clone())
        }

        fn commit(&self, writes: &[Entry<'_>], deletes: &[&str]) -> rusqlite::Result<()> {
            *self.commits.borrow_mut() += 1;
            if self.fail_commit {
                return Err(rusqlite::Error::InvalidQuery);
            }
            for entry in writes {
                self.put(
                    entry.rel_path,
                    entry.stat.clone(),
                    entry.content_hash,
                    entry.snapshot_ns,
                    entry.payload,
                );
            }
            for rel_path in deletes {
                self.entries.borrow_mut().remove(*rel_path);
            }
            Ok(())
        }
    }

    fn stat(mtime_ns: i64) -> FileStat {
        FileStat {
            size: 3,
            mtime_ns,
            ctime_ns: 1,
            inode: 2,
            dev: 4,
        }
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    fn collect_cached(
        cache: &FakeCache,
        fs: &FakeFs,
        rel_paths: &[PathBuf],
        run: &dyn Fn(&Path) -> std::result::Result<String, String>,
    ) -> (Vec<JsonRow>, Vec<String>) {
        let warnings = std::cell::RefCell::new(Vec::new());
        let rows = collect_rows_cached(cache, rel_paths, fs, run, &|m| {
            warnings.borrow_mut().push(m.to_string())
        })
        .unwrap();
        (rows, warnings.into_inner())
    }

    fn ids(rows: &[JsonRow]) -> Vec<i64> {
        rows.iter()
            .map(|r| r.get("id").unwrap().as_i64().unwrap())
            .collect()
    }

    #[test]
    fn collect_rows_cached_parses_a_file_the_cache_does_not_know() {
        let cache = FakeCache::default();
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, warnings) =
            collect_cached(&cache, &fs, &paths(&["a.json"]), &ok(r#"[{"id":1}]"#));

        assert_eq!(ids(&rows), vec![1]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(cache.payloads()["a.json"], r#"[{"id":1}]"#);
    }

    #[test]
    fn collect_rows_cached_serves_an_unchanged_file_without_parsing_or_reading_it() {
        let cache = FakeCache::seeded(&[("a.json", 10, r#"[{"id":9}]"#)]);
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json"]), &|_| {
            panic!("an unchanged file must not reach the parser")
        });

        assert_eq!(ids(&rows), vec![9], "the cached payload is served");
        assert!(
            fs.hashed.borrow().is_empty(),
            "a file outside the racy window is not read: {:?}",
            fs.hashed.borrow(),
        );
    }

    #[test]
    fn collect_rows_cached_leaves_the_cache_alone_when_nothing_changed() {
        let cache = FakeCache::seeded(&[("a.json", 10, r#"[{"id":9}]"#)]);
        let fs = FakeFs::with(&[("a.json", 10)]);

        collect_cached(&cache, &fs, &paths(&["a.json"]), &ok("[]"));

        assert_eq!(
            *cache.commits.borrow(),
            0,
            "an unchanged scan writes nothing at all",
        );
    }

    #[test]
    fn collect_rows_cached_reparses_a_file_whose_stat_moved() {
        let cache = FakeCache::seeded(&[("a.json", 10, r#"[{"id":9}]"#)]);
        let fs = FakeFs::with(&[("a.json", 20)]);

        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json"]), &ok(r#"[{"id":2}]"#));

        assert_eq!(ids(&rows), vec![2]);
        assert_eq!(
            cache.payloads()["a.json"],
            r#"[{"id":2}]"#,
            "the cache learns the new payload",
        );
    }

    #[test]
    fn collect_rows_cached_forgets_a_file_the_scan_no_longer_matches() {
        let cache = FakeCache::seeded(&[
            ("a.json", 10, r#"[{"id":9}]"#),
            ("gone.json", 10, r#"[{"id":8}]"#),
        ]);
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json"]), &ok("[]"));

        assert_eq!(ids(&rows), vec![9]);
        assert_eq!(
            cache.payloads().keys().collect::<Vec<_>>(),
            vec!["a.json"],
            "the vanished file's entry is dropped",
        );
    }

    #[test]
    fn collect_rows_cached_skips_a_file_that_vanished_before_the_stat() {
        let cache = FakeCache::default();
        let fs = FakeFs::default();

        let (rows, warnings) = collect_cached(&cache, &fs, &paths(&["ghost.json"]), &|_| {
            panic!("a vanished file must not reach the parser")
        });

        assert!(rows.is_empty());
        assert!(warnings.is_empty(), "a race is not a parse failure");
        assert_eq!(*cache.commits.borrow(), 0);
    }

    #[test]
    fn collect_rows_cached_isolates_a_parser_failure_and_leaves_it_uncached() {
        let cache = FakeCache::default();
        let fs = FakeFs::with(&[("bad.json", 10), ("good.json", 10)]);

        let (rows, warnings) =
            collect_cached(&cache, &fs, &paths(&["bad.json", "good.json"]), &|rel| {
                if rel == Path::new("bad.json") {
                    Err("exit 7".to_string())
                } else {
                    Ok(r#"[{"id":1}]"#.to_string())
                }
            });

        assert_eq!(ids(&rows), vec![1], "the good file still contributes");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("bad.json"));
        assert!(
            !cache.payloads().contains_key("bad.json"),
            "a failed file stays uncached so the next run retries it",
        );
    }

    #[test]
    fn collect_rows_cached_isolates_output_that_is_not_a_row_array() {
        let cache = FakeCache::default();
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, warnings) = collect_cached(&cache, &fs, &paths(&["a.json"]), &ok("not json"));

        assert!(rows.is_empty());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("a.json"), "got: {}", warnings[0]);
        assert_eq!(
            *cache.commits.borrow(),
            0,
            "unparseable output is not worth caching",
        );
    }

    #[test]
    fn collect_rows_cached_hash_confirms_a_file_inside_the_racy_window() {
        // snapshot_ns == mtime_ns puts the entry inside the racy window, where
        // the stat tuple alone cannot settle it.
        let cache = FakeCache::default();
        cache.put("a.json", stat(10), Some([3u8; 32]), 10, "[]");
        let mut fs = FakeFs::with(&[("a.json", 10)]);
        fs.hashes.insert("a.json".to_string(), [3u8; 32]);

        collect_cached(&cache, &fs, &paths(&["a.json"]), &|_| {
            panic!("a hash-confirmed file must not reach the parser")
        });

        assert_eq!(fs.hashed.borrow().len(), 1, "the file is hashed once");
    }

    #[test]
    fn collect_rows_cached_warns_but_returns_rows_when_the_cache_cannot_be_written() {
        let cache = FakeCache {
            fail_commit: true,
            ..FakeCache::default()
        };
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, warnings) =
            collect_cached(&cache, &fs, &paths(&["a.json"]), &ok(r#"[{"id":1}]"#));

        assert_eq!(ids(&rows), vec![1], "the rows are correct regardless");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("next run re-parses"),
            "the warning says what was lost, got: {}",
            warnings[0],
        );
    }

    #[test]
    fn cache_write_skip_message_names_the_error_and_the_consequence() {
        let message = cache_write_skip_message(&Error::SqliteSingleThreadedMode);
        assert!(message.contains("persist cache"), "got: {message}");
        assert!(message.contains("rows are unaffected"), "got: {message}");
    }

    #[test]
    fn collect_rows_cached_propagates_a_cache_read_failure() {
        let cache = FakeCache {
            fail_read: true,
            ..FakeCache::default()
        };
        let fs = FakeFs::with(&[("a.json", 10)]);
        let err =
            collect_rows_cached(&cache, &paths(&["a.json"]), &fs, &ok("[]"), &|_| {}).unwrap_err();
        assert!(matches!(err, Error::InvalidQuery), "got: {err}");
    }

    // RootedFs answers real filesystem questions, so the isolation rule keeps
    // it out of the unit tier: `tests/persist_parsed_path_table.rs` exercises
    // it against a real tree, the way `vtab.rs`'s `read_text` is covered.
}
