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
use std::sync::Arc;

use globset::GlobSet;
use rusqlite::vtab::Context;
use rusqlite::{Connection, Error, Result};

use crate::Value;
use crate::infer::{JsonRow, cell, declared_schema, infer_schema, parse_rows};
use crate::matcher::TableMatcher;
use crate::on_file;
use crate::parsed_cache::{self, CachedParse, Entry, RowCache, SqliteRowCache};
use crate::persist::{FileStat, hash_file, now_ns};
use crate::scanner::{scan_glob, to_slash};
use crate::vtab_scaffold::{self, StatementScope, TableSource};

/// SQL module name a parsed path-table is created with.
pub const MODULE_NAME: &str = "dirsql_parsed";

/// Register the parsed path-table module on `conn`.
pub fn load_module(conn: &Connection, scope: Arc<StatementScope>) -> Result<()> {
    vtab_scaffold::load_module::<ParsedTable>(conn, scope)
}

/// Number of module arguments that are not ignore patterns.
const FIXED_ARGS: usize = 6;

/// The module's own arguments: scan root, glob pattern, parser command, the
/// gitignore switch, the persistent cache path (empty when the index is
/// ephemeral), the index root the parser is spawned in, and the skip rules
/// the scan applies (mirroring the stat path-table's ignore args).
struct ModuleArgs {
    root: PathBuf,
    pattern: String,
    glob: GlobSet,
    command: String,
    gitignore: bool,
    cache: Option<PathBuf>,
    index_root: PathBuf,
    ignore: TableMatcher,
}

/// Parse a parsed path-table's `CREATE VIRTUAL TABLE` arguments. `args[0..3]`
/// are the module, database and table names; the module's own follow — root,
/// glob, parser, the gitignore switch, the cache path, the index root, then
/// any ignore patterns.
fn parse_module_args(args: &[&[u8]]) -> Result<ModuleArgs> {
    let user_args = vtab_scaffold::user_args(args);

    let [
        root,
        pattern,
        command,
        gitignore,
        cache,
        index_root,
        ignore @ ..,
    ] = user_args.as_slice()
    else {
        return Err(vtab_scaffold::arity_error(
            MODULE_NAME,
            FIXED_ARGS,
            "root, glob, parser, gitignore switch, cache path, index root",
            user_args.len(),
        ));
    };

    Ok(ModuleArgs {
        root: PathBuf::from(root),
        pattern: pattern.clone(),
        glob: vtab_scaffold::compile_glob(pattern)?,
        command: command.clone(),
        gitignore: vtab_scaffold::parse_gitignore(gitignore)?,
        // The empty string is how "no cache" is spelled: a module argument
        // cannot be absent without shifting every argument after it.
        cache: Some(PathBuf::from(cache)).filter(|p| !p.as_os_str().is_empty()),
        index_root: PathBuf::from(index_root),
        ignore: vtab_scaffold::compile_ignore(ignore)?,
    })
}

/// Run the parser over every matched file and concatenate the rows.
///
/// `run` is injected so the fan-out and the ordering can be unit-tested without
/// spawning a process; `warn` is injected so the skip warnings can be captured.
/// Production passes a closure over [`run_parser`] and `|m| eprintln!("{m}")`.
///
/// One run over every file, matching the `on-file` hook contract: the
/// parser's output is the table's rows, so a run that fails or does not parse
/// is the table's error and the schema is inferred from what it emitted.
fn collect_rows(rel_paths: &[PathBuf], run: &RunParser<'_>) -> Result<Vec<JsonRow>> {
    run(rel_paths).map_err(Error::ModuleError)
}

/// The cache entry holding the whole table's rows. The per-file entries carry
/// only the stat tuples the freshness check reads; the parser ran over every
/// file at once, so there is no per-file payload to keep.
const TABLE_ENTRY: &str = "";

/// The parser over a table's files: every root-relative path in, the table's
/// rows out, or the message to fail the table with.
type RunParser<'a> = dyn Fn(&[PathBuf]) -> std::result::Result<Vec<JsonRow>, String> + 'a;

/// [`collect_rows`] with the persistent cache in front of the parser: when
/// every matched file's stat tuple matches its cached entry and no cached file
/// has gone, the table's cached rows are served and the parser never runs.
/// Any one change re-runs it over every file, since that is what it saw.
///
/// Everything effectful is injected — `stat`, `hash`, `run`, `warn` — so the
/// reuse decision can be exercised without a filesystem or a child process.
/// `hash` is only ever called for a file the stat tuple alone cannot settle,
/// which is what keeps an unchanged tree free of file reads.
///
/// The cache is only written when the scan actually changed something, which is
/// what lets an unchanged tree leave the cache file byte-for-byte alone. A
/// failed parse is the table's error and leaves the cache untouched.
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
    run: &RunParser<'_>,
    warn: &dyn Fn(&str),
) -> Result<Vec<JsonRow>> {
    let cached: HashMap<String, CachedParse> = cache.read()?;
    let snapshot_ns = now_ns();

    let mut present: Vec<PathBuf> = Vec::with_capacity(rel_paths.len());
    let mut changed: Vec<(String, FileStat)> = Vec::new();
    for rel_path in rel_paths {
        let key = to_slash(rel_path);
        let Some(live) = fs.stat(rel_path) else {
            // The file vanished between the scan and the stat. Nothing to
            // parse and nothing to cache; the next run decides afresh.
            continue;
        };
        if !parsed_cache::is_fresh(cached.get(&key), &live, || fs.hash(rel_path)) {
            changed.push((key.clone(), live));
        }
        present.push(PathBuf::from(key));
    }

    let live: HashSet<String> = present.iter().map(|path| to_slash(path)).collect();
    let stale: Vec<&str> = cached
        .keys()
        .map(String::as_str)
        .filter(|key| *key != TABLE_ENTRY && !live.contains(*key))
        .collect();

    if changed.is_empty()
        && stale.is_empty()
        && let Some(table) = cached.get(TABLE_ENTRY)
    {
        return parse_rows(&table.payload).map_err(Error::ModuleError);
    }

    let rows = collect_rows(&present, run)?;
    let payload = serde_json::to_string(&rows).map_err(|e| Error::ModuleError(e.to_string()))?;
    let mut writes: Vec<Entry<'_>> = changed
        .iter()
        .map(|(key, stat)| Entry {
            rel_path: key,
            stat,
            content_hash: fs.hash(Path::new(key)),
            snapshot_ns,
            payload: "",
        })
        .collect();
    let table_stat = FileStat {
        size: 0,
        mtime_ns: 0,
        ctime_ns: 0,
        inode: 0,
        dev: 0,
    };
    writes.push(Entry {
        rel_path: TABLE_ENTRY,
        stat: &table_stat,
        content_hash: None,
        snapshot_ns,
        payload: &payload,
    });
    if let Err(error) = cache.commit(&writes, &stale) {
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

/// The error raised when the sample yields nothing to infer from. SQLite has
/// no zero-column table, and inventing a placeholder column would make
/// `SELECT *` mean something the parser never said.
fn no_rows_message(pattern: &str) -> String {
    format!("{MODULE_NAME}: parser produced no rows for `{pattern}`; cannot infer a schema")
}

/// Run the parser once over `rel_paths`, handing it their absolute paths as
/// trailing arguments, from the index root as its working directory, with
/// `{root}` naming it, matching the `on-file` contract.
fn run_parser(
    command: &str,
    index_root: &Path,
    root: &Path,
    rel_paths: &[PathBuf],
) -> std::result::Result<Vec<JsonRow>, String> {
    let abs_paths: Vec<PathBuf> = rel_paths.iter().map(|rel| root.join(rel)).collect();
    on_file::run(command, index_root, index_root, &abs_paths)
}

/// Column name for an index, or `None` when SQLite asks for one out of range.
fn column_name(names: &[String], i: c_int) -> Option<&str> {
    let index = usize::try_from(i).ok()?;
    names.get(index).map(String::as_str)
}

/// The table a `CREATE` produced: the inferred column names and the rows the
/// parsers emitted, both fixed for the life of the table.
struct ParsedTable {
    /// Only the names survive registration: the types live in the schema
    /// SQLite already holds, and a cursor addresses columns by index.
    column_names: Vec<String>,
    rows: Arc<Vec<JsonRow>>,
}

impl TableSource for ParsedTable {
    type Row = JsonRow;

    const NAME: &'static str = MODULE_NAME;

    fn connect(args: &[&[u8]]) -> Result<(String, Self)> {
        let ModuleArgs {
            root,
            pattern,
            glob,
            command,
            gitignore,
            cache,
            index_root,
            ignore,
        } = parse_module_args(args)?;

        // A parsed path-table honors the same skip rules a stat path-table does
        // (node_modules/.git, gitignore, plus any configured ignore), so a
        // parsed `SELECT * FROM './'` doesn't drown in dependency trees.
        let rel_paths = scan_glob(&root, &glob, &ignore, gitignore);
        let run = |rel: &[PathBuf]| run_parser(&command, &index_root, &root, rel);
        let warn = |message: &str| eprintln!("{message}");
        let rows = match &cache {
            None => collect_rows(&rel_paths, &run)?,
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
        let table = Self {
            column_names: columns.into_iter().map(|c| c.name).collect(),
            rows: Arc::new(rows),
        };
        Ok((schema, table))
    }

    fn rows(&self) -> Arc<Vec<JsonRow>> {
        // Materialized once at CREATE, so every statement reads the same rows
        // rather than re-spawning a parser per file.
        Arc::clone(&self.rows)
    }

    fn column(&self, row: &JsonRow, ctx: &mut Context, i: c_int) -> Result<()> {
        let value = column_name(&self.column_names, i)
            .map(|name| cell(row, name))
            .unwrap_or(Value::Null);
        ctx.set_result(&value)
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
    fn parse_module_args_extracts_root_glob_command_and_index_root() {
        let args = args_with(&[
            b"'/tmp/notes/docs'",
            b"'**/*.json'",
            b"'cat {path}'",
            b"'gitignore'",
            b"''",
            b"'/tmp/notes'",
        ]);
        let parsed = parse_module_args(&args).unwrap();

        assert_eq!(parsed.root, PathBuf::from("/tmp/notes/docs"));
        assert_eq!(parsed.pattern, "**/*.json");
        assert_eq!(parsed.command, "cat {path}");
        assert_eq!(parsed.index_root, PathBuf::from("/tmp/notes"));
        assert!(parsed.glob.is_match(Path::new("a.json")));
        assert!(!parsed.glob.is_match(Path::new("a.md")));
    }

    #[test]
    fn parse_module_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes", b"**/*", b"cat", b"gitignore", b"", b"/tmp"]);
        assert_eq!(
            parse_module_args(&args).unwrap().root,
            PathBuf::from("/tmp/notes")
        );
    }

    #[test]
    fn parse_module_args_reads_the_gitignore_switch() {
        let on = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'/tmp'",
        ]);
        assert!(parse_module_args(&on).unwrap().gitignore);

        let off = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'no-gitignore'",
            b"''",
            b"'/tmp'",
        ]);
        assert!(!parse_module_args(&off).unwrap().gitignore);
    }

    #[test]
    fn parse_module_args_rejects_an_unknown_gitignore_switch() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'sometimes'",
            b"''",
            b"'/tmp'",
        ]);
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
            b"'/tmp'",
            b"'node_modules/**'",
        ]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
        assert!(!parsed.ignore.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn parse_module_args_with_no_ignore_patterns_ignores_nothing() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'/tmp'",
        ]);
        let parsed = parse_module_args(&args).unwrap();
        assert!(!parsed.ignore.is_ignored(Path::new("node_modules/pkg/a.js")));
    }

    #[test]
    fn parse_module_args_rejects_too_few_arguments() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"'cat'", b"'gitignore'", b"''"]);
        let err = match parse_module_args(&args) {
            Err(err) => err,
            Ok(_) => panic!("five arguments must be rejected"),
        };
        assert!(
            err.to_string().contains("at least"),
            "error should name the arity, got: {err}"
        );
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_glob() {
        let args = args_with(&[
            b"'/tmp'",
            b"'['",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'/tmp'",
        ]);
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
            b"'/tmp'",
        ]);
        assert_eq!(
            parse_module_args(&args).unwrap().cache,
            Some(PathBuf::from("/cache/dirsql.db"))
        );
    }

    #[test]
    fn parse_module_args_reads_an_empty_cache_path_as_no_cache() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"'cat'",
            b"'gitignore'",
            b"''",
            b"'/tmp'",
        ]);
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
            b"'/tmp'",
            b"'['",
        ]);
        assert!(parse_module_args(&args).is_err());
    }

    fn ok(
        payload: &'static str,
    ) -> impl Fn(&[PathBuf]) -> std::result::Result<Vec<JsonRow>, String> {
        move |_| parse_rows(payload)
    }

    #[test]
    fn collect_rows_hands_every_file_to_one_run_in_scan_order() {
        let paths = vec![PathBuf::from("a.json"), PathBuf::from("b.json")];
        let seen = std::cell::RefCell::new(Vec::new());
        let rows = collect_rows(&paths, &|rel| {
            seen.borrow_mut().push(rel.to_vec());
            parse_rows(r#"[{"i":1},{"i":2}]"#)
        })
        .unwrap();

        assert_eq!(rows.len(), 2);
        assert_eq!(
            *seen.borrow(),
            vec![paths.clone()],
            "one run over all files"
        );
    }

    #[test]
    fn collect_rows_over_no_files_hands_the_run_no_paths() {
        let rows = collect_rows(&[], &|rel| {
            assert!(rel.is_empty());
            Ok(Vec::new())
        })
        .unwrap();
        assert_eq!(rows, Vec::new());
    }

    #[test]
    fn collect_rows_surfaces_a_run_failure_as_the_tables_error() {
        let paths = vec![PathBuf::from("a.json")];
        let err = collect_rows(&paths, &|_| Err("exit 7".to_string())).unwrap_err();
        assert!(
            matches!(err, Error::ModuleError(ref m) if m == "exit 7"),
            "got: {err}"
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

    // The run_parser tests spawn a real `sh`. Their test code statically
    // references only `super::` items (plus pure std), matching the pattern
    // `command.rs` uses for the runner underneath.

    #[test]
    fn run_parser_returns_the_commands_rows() {
        let rows = run_parser(
            r#"sh -c "echo chatter; printf '[{\"id\":1}]'""#,
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("a.json")],
        )
        .unwrap();
        assert_eq!(ids(&rows), vec![1]);
    }

    /// `{root}` and every absolute path reach the command as arguments, in
    /// order. The command writes them to a side file rather than into the
    /// JSON payload, where a Windows path's `\` would need escaping.
    #[test]
    fn run_parser_appends_every_absolute_path_after_the_index_root() {
        let index_root =
            std::env::temp_dir().join(format!("dirsql-run-parser-{}", std::process::id()));
        let root = index_root.join("docs");
        std::fs::create_dir_all(&root).unwrap();
        run_parser(
            r#"sh -c 'printf "%s\n%s\n%s\n" "$1" "$2" "$3" > seen; echo "[]"' sh {root}"#,
            &index_root,
            &root,
            &[PathBuf::from("a.json"), PathBuf::from("b.json")],
        )
        .unwrap();
        let seen = std::fs::read_to_string(index_root.join("seen")).unwrap();
        std::fs::remove_dir_all(&index_root).unwrap();
        assert_eq!(
            seen,
            format!(
                "{}\n{}\n{}\n",
                index_root.display(),
                root.join("a.json").display(),
                root.join("b.json").display()
            ),
            "the side file lands in the index root, which is the working directory"
        );
    }

    #[test]
    fn run_parser_surfaces_a_command_failure_as_a_message() {
        let err = run_parser(
            "sh -c 'exit 7'",
            Path::new("."),
            Path::new("."),
            &[PathBuf::from("a.json")],
        )
        .unwrap_err();
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
        fn seeded(files: &[(&str, i64)], rows: &str) -> Self {
            let cache = Self::default();
            for (rel_path, mtime_ns) in files {
                cache.put(rel_path, stat(*mtime_ns), None, mtime_ns + 1, "");
            }
            cache.put(TABLE_ENTRY, stat(0), None, 1, rows);
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
        run: &RunParser<'_>,
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
        assert_eq!(cache.payloads()[TABLE_ENTRY], r#"[{"id":1}]"#);
        assert!(cache.payloads().contains_key("a.json"));
    }

    #[test]
    fn collect_rows_cached_serves_an_unchanged_tree_without_parsing_or_reading_it() {
        let cache = FakeCache::seeded(&[("a.json", 10)], r#"[{"id":9}]"#);
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json"]), &|_| {
            panic!("an unchanged tree must not reach the parser")
        });

        assert_eq!(ids(&rows), vec![9], "the cached rows are served");
        assert!(
            fs.hashed.borrow().is_empty(),
            "a file outside the racy window is not read: {:?}",
            fs.hashed.borrow(),
        );
    }

    #[test]
    fn collect_rows_cached_leaves_the_cache_alone_when_nothing_changed() {
        let cache = FakeCache::seeded(&[("a.json", 10)], r#"[{"id":9}]"#);
        let fs = FakeFs::with(&[("a.json", 10)]);

        collect_cached(&cache, &fs, &paths(&["a.json"]), &ok("[]"));

        assert_eq!(
            *cache.commits.borrow(),
            0,
            "an unchanged scan writes nothing at all",
        );
    }

    #[test]
    fn collect_rows_cached_reruns_over_every_file_when_one_stat_moved() {
        let cache = FakeCache::seeded(&[("a.json", 10), ("b.json", 10)], r#"[{"id":9}]"#);
        let fs = FakeFs::with(&[("a.json", 20), ("b.json", 10)]);

        let seen = std::cell::RefCell::new(Vec::new());
        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json", "b.json"]), &|rel| {
            seen.borrow_mut().push(rel.to_vec());
            parse_rows(r#"[{"id":2}]"#)
        });

        assert_eq!(ids(&rows), vec![2]);
        assert_eq!(
            *seen.borrow(),
            vec![paths(&["a.json", "b.json"])],
            "the parser sees the whole table again",
        );
        assert_eq!(
            cache.payloads()[TABLE_ENTRY],
            r#"[{"id":2}]"#,
            "the cache learns the new rows",
        );
    }

    #[test]
    fn collect_rows_cached_reruns_when_a_cached_file_no_longer_matches() {
        let cache = FakeCache::seeded(&[("a.json", 10), ("gone.json", 10)], r#"[{"id":9}]"#);
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, _) = collect_cached(&cache, &fs, &paths(&["a.json"]), &ok(r#"[{"id":1}]"#));

        assert_eq!(ids(&rows), vec![1]);
        assert_eq!(
            cache.payloads().keys().collect::<Vec<_>>(),
            vec![TABLE_ENTRY, "a.json"],
            "the vanished file's entry is dropped",
        );
    }

    #[test]
    fn collect_rows_cached_leaves_out_a_file_that_vanished_before_the_stat() {
        let cache = FakeCache::default();
        let fs = FakeFs::with(&[("a.json", 10)]);

        let (rows, warnings) =
            collect_cached(&cache, &fs, &paths(&["ghost.json", "a.json"]), &|rel| {
                assert_eq!(
                    rel,
                    paths(&["a.json"]),
                    "the vanished file is not handed over"
                );
                parse_rows(r#"[{"id":1}]"#)
            });

        assert_eq!(ids(&rows), vec![1]);
        assert!(warnings.is_empty(), "a race is not a parse failure");
    }

    #[test]
    fn collect_rows_cached_propagates_a_parser_failure_and_leaves_the_cache_alone() {
        let cache = FakeCache::default();
        let fs = FakeFs::with(&[("a.json", 10)]);

        let err = collect_rows_cached(
            &cache,
            &paths(&["a.json"]),
            &fs,
            &|_| Err("exit 7".to_string()),
            &|_| {},
        )
        .unwrap_err();

        assert!(
            matches!(err, Error::ModuleError(ref m) if m == "exit 7"),
            "got: {err}"
        );
        assert_eq!(*cache.commits.borrow(), 0, "a failed run is not cached");
    }

    #[test]
    fn collect_rows_cached_hash_confirms_a_file_inside_the_racy_window() {
        // snapshot_ns == mtime_ns puts the entry inside the racy window, where
        // the stat tuple alone cannot settle it.
        let cache = FakeCache::seeded(&[], "[]");
        cache.put("a.json", stat(10), Some([3u8; 32]), 10, "");
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
