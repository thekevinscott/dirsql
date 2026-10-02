use std::ffi::c_int;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use globset::GlobSet;
use rusqlite::types::Null;
use rusqlite::vtab::Context;
use rusqlite::{Connection, Result};

use crate::matcher::TableMatcher;
use crate::path_table;
use crate::scanner::{scan_glob, to_slash};
use crate::vtab_scaffold::{self, TableSource};

pub use crate::vtab_scaffold::StatementScope;

/// SQL module name a path-table is created with:
/// `CREATE VIRTUAL TABLE t USING dirsql_path('<root>', '<glob>', '<path prefix>',
/// '<gitignore|no-gitignore>'[, '<ignore>'...])`.
pub const MODULE_NAME: &str = "dirsql_path";

/// Number of module arguments that are not ignore patterns.
const FIXED_ARGS: usize = 4;

/// The seven stat columns, in declaration order.
pub const STAT_COLUMNS: [&str; 7] = ["path", "basename", "dir", "ext", "size", "mtime", "ctime"];

const PATH_COLUMN: usize = 0;
const BASENAME_COLUMN: usize = 1;
const DIR_COLUMN: usize = 2;
const EXT_COLUMN: usize = 3;
const SIZE_COLUMN: usize = 4;
const MTIME_COLUMN: usize = 5;
const CTIME_COLUMN: usize = 6;

/// Column index of the lazily-read `content`, which follows the stat columns.
const CONTENT_COLUMN: usize = STAT_COLUMNS.len();

/// Schema a path-table declares to SQLite.
///
/// `content` is declared last and `HIDDEN` so SQLite excludes it from
/// `SELECT *` while still resolving it by name. That is what makes laziness
/// structural rather than aspirational: a bare `SELECT *` never asks for the
/// column, so the file body is never read.
pub fn declared_schema() -> String {
    let stats = STAT_COLUMNS
        .iter()
        .map(|c| format!("{c} {}", column_type(c)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("CREATE TABLE x({stats}, content TEXT HIDDEN)")
}

/// Declared SQLite type for a stat column.
fn column_type(column: &str) -> &'static str {
    match column {
        "size" | "mtime" | "ctime" => "INTEGER",
        _ => "TEXT",
    }
}

/// Read `path` as text, yielding `None` when it is unreadable or not valid
/// UTF-8. A file that cannot be read is a NULL cell, never a failed row: the
/// filesystem is allowed to be messy and a query over it should still return.
fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}

/// Everything a scan needs: where to walk, what to match, what to call the
/// results, and what to skip.
struct ScanSpec {
    root: PathBuf,
    glob: GlobSet,
    /// Prepended to each matched path before the stat columns are computed.
    /// Empty for index-root-relative tables.
    path_prefix: PathBuf,
    ignore: TableMatcher,
    /// Literal directories the pattern named outright; skip rules are judged
    /// below this.
    ignore_base: PathBuf,
    /// Whether the scan respects `.gitignore` files (off under `--no-ignore`).
    gitignore: bool,
}

/// One matched file, as compact as the seven stat columns allow: the three
/// path-derived text columns are slices of `path` located by [`PathSpans`],
/// so a row costs two allocations however many columns a query reads.
struct FileRow {
    /// The path as scanned, root-relative; where the lazy content read goes.
    rel: PathBuf,
    /// The path as reported, under the table's prefix when it has one.
    path: String,
    spans: PathSpans,
    size: Option<i64>,
    mtime: Option<i64>,
    ctime: Option<i64>,
}

impl FileRow {
    fn new(path_prefix: &Path, rel: PathBuf, metadata: Option<&Metadata>) -> Self {
        let path = reported_path(path_prefix, &rel);
        let spans = PathSpans::of(&path);
        let (size, mtime, ctime) = stat_facts(metadata);
        Self {
            rel,
            path,
            spans,
            size,
            mtime,
            ctime,
        }
    }

    fn basename(&self) -> Option<&str> {
        self.spans.name_at.map(|at| &self.path[at..])
    }

    fn dir(&self) -> Option<&str> {
        self.spans.dir_len.map(|len| &self.path[..len])
    }

    fn ext(&self) -> Option<&str> {
        self.spans.ext_at.map(|at| &self.path[at..])
    }
}

/// Where `basename`, `dir` and `ext` sit inside a reported path, each `None`
/// where [`Path`] has no answer: no basename or dir for an empty path, no ext
/// for a dotfile or an extensionless name. [`Path::parent`] is a prefix of
/// the path and [`Path::file_name`] and [`Path::extension`] are suffixes, so
/// one length each locates them.
#[derive(Debug, PartialEq, Eq)]
struct PathSpans {
    name_at: Option<usize>,
    dir_len: Option<usize>,
    ext_at: Option<usize>,
}

impl PathSpans {
    fn of(path: &str) -> Self {
        let p = Path::new(path);
        let suffix_at = |s: &std::ffi::OsStr| path.len() - s.len();
        Self {
            name_at: p.file_name().map(suffix_at),
            dir_len: p.parent().map(|d| d.as_os_str().len()),
            ext_at: p.extension().map(suffix_at),
        }
    }
}

/// The stat-derived columns, each `None` when the file could not be stat'ed
/// or the platform cannot supply the fact (or it predates the epoch).
fn stat_facts(metadata: Option<&Metadata>) -> (Option<i64>, Option<i64>, Option<i64>) {
    let Some(metadata) = metadata else {
        return (None, None, None);
    };
    (
        i64::try_from(metadata.len()).ok(),
        epoch_secs(metadata.modified().ok()),
        epoch_secs(metadata.created().ok()),
    )
}

fn epoch_secs(time: Option<SystemTime>) -> Option<i64> {
    let since = time?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

/// The rows for the files a scan found, in scan order. The stat behind each
/// row is a syscall the kernel answers independently of the last, so the
/// files are shared out across every core: one stat per file is the floor
/// `find` pays too, and spreading them is how a walk comes in under it.
fn build_rows(root: &Path, path_prefix: &Path, rel_paths: Vec<PathBuf>) -> Vec<FileRow> {
    let workers = thread::available_parallelism().map_or(1, usize::from);
    let per_worker = rel_paths.len().div_ceil(workers).max(1);
    let row = |rel: PathBuf| {
        let metadata = fs::metadata(root.join(&rel)).ok();
        FileRow::new(path_prefix, rel, metadata.as_ref())
    };
    if rel_paths.len() <= per_worker {
        return rel_paths.into_iter().map(row).collect();
    }
    thread::scope(|scope| {
        let mut chunks: Vec<Vec<PathBuf>> = Vec::with_capacity(workers);
        let mut rest = rel_paths;
        while rest.len() > per_worker {
            let tail = rest.split_off(per_worker);
            chunks.push(std::mem::replace(&mut rest, tail));
        }
        chunks.push(rest);
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || chunk.into_iter().map(row).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a stat worker only stats"))
            .collect()
    })
}

/// Parse a path-table's own `CREATE VIRTUAL TABLE` arguments into its scan
/// spec. `args[0..3]` are the module, database and table names; the module's
/// own arguments follow — root, glob, path prefix, the gitignore switch, then
/// any ignore patterns.
fn parse_module_args(args: &[&[u8]]) -> Result<ScanSpec> {
    let user_args = vtab_scaffold::user_args(args);

    let [root, pattern, path_prefix, gitignore, ignore @ ..] = user_args.as_slice() else {
        return Err(vtab_scaffold::arity_error(
            MODULE_NAME,
            FIXED_ARGS,
            "root, glob, path prefix, gitignore switch",
            user_args.len(),
        ));
    };

    Ok(ScanSpec {
        root: PathBuf::from(root),
        glob: vtab_scaffold::compile_glob(pattern)?,
        path_prefix: PathBuf::from(path_prefix),
        ignore: vtab_scaffold::compile_ignore(ignore)?,
        ignore_base: path_table::ignore_base(pattern),
        gitignore: vtab_scaffold::parse_gitignore(gitignore)?,
    })
}

/// The string a matched file is reported under: the relative path as scanned,
/// under the table's path prefix when it has one.
fn reported_path(path_prefix: &Path, rel_path: &Path) -> String {
    let rel_path = to_slash(rel_path);
    let prefix = path_prefix.to_string_lossy();
    if prefix.is_empty() {
        return rel_path;
    }
    if prefix.ends_with(['/', std::path::MAIN_SEPARATOR]) {
        return format!("{prefix}{rel_path}");
    }
    format!("{prefix}/{rel_path}")
}

impl TableSource for ScanSpec {
    type Row = FileRow;

    const NAME: &'static str = MODULE_NAME;

    fn connect(args: &[&[u8]]) -> Result<(String, Self)> {
        Ok((declared_schema(), parse_module_args(args)?))
    }

    fn rows(&self) -> Arc<Vec<FileRow>> {
        // The scan runs per statement rather than at CREATE, which is what
        // makes reads live: each statement sees the filesystem as it is now.
        let rel_paths = scan_glob(
            &self.root,
            &self.glob,
            &self.ignore,
            &self.ignore_base,
            self.gitignore,
        );
        Arc::new(build_rows(&self.root, &self.path_prefix, rel_paths))
    }

    fn column(&self, row: &FileRow, ctx: &mut Context, i: c_int) -> Result<()> {
        match usize::try_from(i).unwrap_or(usize::MAX) {
            PATH_COLUMN => ctx.set_result(&row.path.as_str()),
            BASENAME_COLUMN => ctx.set_result(&row.basename()),
            DIR_COLUMN => ctx.set_result(&row.dir()),
            EXT_COLUMN => ctx.set_result(&row.ext()),
            SIZE_COLUMN => ctx.set_result(&row.size),
            MTIME_COLUMN => ctx.set_result(&row.mtime),
            CTIME_COLUMN => ctx.set_result(&row.ctime),
            // The one effectful read, reached only when a query names the
            // column: this is where laziness actually lives.
            CONTENT_COLUMN => ctx.set_result(&read_text(&self.root.join(&row.rel))),
            _ => ctx.set_result(&Null),
        }
    }
}

/// Register the path-table module on `conn`. Its tables hold the rows of the
/// statement in progress under `scope`; see [`StatementScope`].
pub fn load_module(conn: &Connection, scope: Arc<StatementScope>) -> Result<()> {
    vtab_scaffold::load_module::<ScanSpec>(conn, scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real vtab behavior over a real directory is covered by `tests/vtab.rs`
    // (unit-lint isolation); only the pure helpers are tested here.

    #[test]
    fn stat_columns_are_the_seven_documented_names() {
        assert_eq!(
            STAT_COLUMNS,
            ["path", "basename", "dir", "ext", "size", "mtime", "ctime"]
        );
    }

    #[test]
    fn column_indices_follow_the_declared_order() {
        assert_eq!(STAT_COLUMNS[PATH_COLUMN], "path");
        assert_eq!(STAT_COLUMNS[BASENAME_COLUMN], "basename");
        assert_eq!(STAT_COLUMNS[DIR_COLUMN], "dir");
        assert_eq!(STAT_COLUMNS[EXT_COLUMN], "ext");
        assert_eq!(STAT_COLUMNS[SIZE_COLUMN], "size");
        assert_eq!(STAT_COLUMNS[MTIME_COLUMN], "mtime");
        assert_eq!(STAT_COLUMNS[CTIME_COLUMN], "ctime");
    }

    #[test]
    fn declared_schema_lists_every_stat_column() {
        let schema = declared_schema();
        for col in STAT_COLUMNS {
            assert!(schema.contains(col), "{col} missing from {schema}");
        }
    }

    #[test]
    fn declared_schema_marks_content_hidden_and_last() {
        let schema = declared_schema();
        assert!(
            schema.contains("content TEXT HIDDEN"),
            "content must be HIDDEN: {schema}"
        );
        assert!(
            schema.find("content").unwrap() > schema.find("ctime").unwrap(),
            "content must follow the stat columns: {schema}"
        );
    }

    #[test]
    fn content_column_index_follows_the_stat_columns() {
        assert_eq!(CONTENT_COLUMN, 7);
    }

    #[test]
    fn numeric_stat_columns_declare_integer() {
        assert_eq!(column_type("size"), "INTEGER");
        assert_eq!(column_type("mtime"), "INTEGER");
        assert_eq!(column_type("ctime"), "INTEGER");
    }

    #[test]
    fn path_like_stat_columns_declare_text() {
        assert_eq!(column_type("path"), "TEXT");
        assert_eq!(column_type("basename"), "TEXT");
        assert_eq!(column_type("dir"), "TEXT");
        assert_eq!(column_type("ext"), "TEXT");
    }

    #[test]
    fn module_name_is_stable() {
        assert_eq!(MODULE_NAME, "dirsql_path");
    }

    /// The three fixed arguments SQLite prepends before the module's own.
    fn args_with<'a>(user: &[&'a [u8]]) -> Vec<&'a [u8]> {
        let mut all: Vec<&'a [u8]> = vec![
            b"dirsql_path".as_slice(),
            b"main".as_slice(),
            b"t".as_slice(),
        ];
        all.extend_from_slice(user);
        all
    }

    #[test]
    fn parse_module_args_extracts_root_and_glob() {
        let args = args_with(&[b"'/tmp/notes'", b"'**/*.md'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();

        assert_eq!(spec.root, PathBuf::from("/tmp/notes"));
        assert!(spec.glob.is_match(Path::new("a.md")));
        assert!(!spec.glob.is_match(Path::new("a.csv")));
    }

    #[test]
    fn parse_module_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes", b"**/*", b"", b"gitignore"]);
        assert_eq!(
            parse_module_args(&args).unwrap().root,
            PathBuf::from("/tmp/notes")
        );
    }

    #[test]
    fn parse_module_args_extracts_the_path_prefix() {
        let args = args_with(&[b"'/var/log'", b"'*.log'", b"'/var/log'", b"'gitignore'"]);
        assert_eq!(
            parse_module_args(&args).unwrap().path_prefix,
            PathBuf::from("/var/log")
        );
    }

    #[test]
    fn parse_module_args_derives_the_ignore_base_from_the_glob() {
        let args = args_with(&[b"'/tmp'", b"'docs/**/*'", b"''", b"'gitignore'"]);
        assert_eq!(
            parse_module_args(&args).unwrap().ignore_base,
            PathBuf::from("docs")
        );
    }

    #[test]
    fn parse_module_args_reads_the_gitignore_switch() {
        let on = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'"]);
        assert!(parse_module_args(&on).unwrap().gitignore);

        let off = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'no-gitignore'"]);
        assert!(!parse_module_args(&off).unwrap().gitignore);
    }

    #[test]
    fn parse_module_args_rejects_an_unknown_gitignore_switch() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'sometimes'"]);
        let err = parse_module_args(&args)
            .err()
            .expect("an unknown switch must be rejected");
        assert!(err.to_string().contains("no-gitignore"), "got: {err}");
    }

    #[test]
    fn parse_module_args_compiles_trailing_ignore_patterns() {
        let args = args_with(&[
            b"'/tmp'",
            b"'**/*'",
            b"''",
            b"'gitignore'",
            b"'node_modules/**'",
        ]);
        let spec = parse_module_args(&args).unwrap();

        assert!(spec.ignore.is_ignored(Path::new("node_modules/a.js")));
        assert!(!spec.ignore.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn parse_module_args_accepts_no_ignore_patterns() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();
        assert!(!spec.ignore.is_ignored(Path::new("node_modules/a.js")));
    }

    #[test]
    fn parse_module_args_rejects_too_few_arguments() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''"]);
        let err = parse_module_args(&args)
            .err()
            .expect("arity must be enforced");
        assert!(
            err.to_string().contains("at least 4 arguments"),
            "error should name the arity, got: {err}"
        );
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_glob() {
        let args = args_with(&[b"'/tmp'", b"'['", b"''", b"'gitignore'"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn parse_module_args_rejects_an_invalid_ignore_pattern() {
        let args = args_with(&[b"'/tmp'", b"'**/*'", b"''", b"'gitignore'", b"'['"]);
        assert!(parse_module_args(&args).is_err());
    }

    #[test]
    fn reported_path_is_the_relative_path_without_a_prefix() {
        assert_eq!(
            reported_path(Path::new(""), Path::new("docs/a.md")),
            "docs/a.md"
        );
    }

    #[test]
    fn reported_path_is_absolute_under_a_prefix() {
        assert_eq!(
            reported_path(Path::new("/var/log"), Path::new("a.log")),
            "/var/log/a.log"
        );
    }

    #[test]
    fn reported_path_under_the_filesystem_root_has_one_separator() {
        assert_eq!(reported_path(Path::new("/"), Path::new("a.log")), "/a.log");
    }

    fn row_for(prefix: &str, rel: &str) -> FileRow {
        FileRow::new(Path::new(prefix), PathBuf::from(rel), None)
    }

    #[test]
    fn a_nested_path_splits_into_dir_basename_and_ext() {
        let row = row_for("", "nested/sub.txt");
        assert_eq!(row.path, "nested/sub.txt");
        assert_eq!(row.dir(), Some("nested"));
        assert_eq!(row.basename(), Some("sub.txt"));
        assert_eq!(row.ext(), Some("txt"));
    }

    #[test]
    fn a_top_level_file_has_an_empty_dir() {
        let row = row_for("", "a.md");
        assert_eq!(row.dir(), Some(""));
        assert_eq!(row.basename(), Some("a.md"));
    }

    #[test]
    fn an_absolute_row_reports_its_prefix_in_path_and_dir() {
        let row = row_for("/var/log", "a.log");
        assert_eq!(row.path, "/var/log/a.log");
        assert_eq!(row.dir(), Some("/var/log"));
        assert_eq!(row.basename(), Some("a.log"));
        assert_eq!(
            row.rel,
            PathBuf::from("a.log"),
            "content is read from the scanned path"
        );
    }

    #[test]
    fn a_file_at_the_filesystem_root_has_the_root_as_its_dir() {
        assert_eq!(row_for("/", "a.md").dir(), Some("/"));
    }

    #[test]
    fn a_dotfile_and_a_bare_name_have_no_ext() {
        assert_eq!(row_for("", ".bashrc").ext(), None);
        assert_eq!(row_for("", "Makefile").ext(), None);
    }

    #[test]
    fn a_trailing_dot_is_an_empty_ext() {
        assert_eq!(row_for("", "a.").ext(), Some(""));
    }

    #[test]
    fn ext_keeps_its_case() {
        assert_eq!(row_for("", "Photo.JPG").ext(), Some("JPG"));
    }

    #[test]
    fn an_empty_path_has_neither_basename_nor_dir() {
        assert_eq!(
            PathSpans::of(""),
            PathSpans {
                name_at: None,
                dir_len: None,
                ext_at: None,
            }
        );
    }

    #[test]
    fn an_unstattable_file_has_null_facts_but_is_still_a_row() {
        let row = row_for("", "bare");
        assert_eq!(row.basename(), Some("bare"));
        assert_eq!((row.size, row.mtime, row.ctime), (None, None, None));
    }

    #[test]
    fn stat_facts_are_absent_without_metadata() {
        assert_eq!(stat_facts(None), (None, None, None));
    }

    #[test]
    fn epoch_secs_truncates_to_whole_seconds() {
        let t = UNIX_EPOCH + std::time::Duration::from_millis(100_900);
        assert_eq!(epoch_secs(Some(t)), Some(100));
    }

    #[test]
    fn epoch_secs_is_absent_before_the_epoch_or_without_a_time() {
        let t = UNIX_EPOCH - std::time::Duration::from_secs(1);
        assert_eq!(epoch_secs(Some(t)), None);
        assert_eq!(epoch_secs(None), None);
    }
}
