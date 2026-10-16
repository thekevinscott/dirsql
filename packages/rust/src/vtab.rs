use std::ffi::c_int;
use std::fs::{self, File};
use std::io::{self, Read};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::vtab::Context;
use rusqlite::{Connection, Result, ffi};

use crate::matcher::TableMatcher;
use crate::row_stream::RowStream;
use crate::scanner::{PathGlob, scan_glob_rows, to_slash};
use crate::vtab_scaffold::{self, TableSource};

pub use crate::vtab_scaffold::StatementScope;

/// SQL module name a path-table is created with:
/// `CREATE VIRTUAL TABLE t USING dirsql_path('<root>', '<glob>', '<path prefix>',
/// '<gitignore|no-gitignore>'[, '<ignore>'...])`.
pub const MODULE_NAME: &str = "dirsql_path";

/// Rows a scan hands a reading cursor at once: few enough that the cursor
/// starts long before the walk ends, enough that handing over costs nothing.
const STREAM_CHUNK_ROWS: usize = 4096;

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

/// `path`, `basename` and `dir`: the columns a join between two path-tables
/// is written on, answered by lookup rather than a rescan of the inner glob.
const LOOKUP_COLUMNS: [usize; 3] = [PATH_COLUMN, BASENAME_COLUMN, DIR_COLUMN];

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

/// Files read at once when `content` is read ahead. Each read is a few
/// syscalls the kernel answers independently of the last, so they overlap
/// almost perfectly; past eight the box, not the count, sets the pace.
const READERS: usize = 8;

/// Read `path` as text, yielding `None` when it is unreadable or not valid
/// UTF-8. A file that cannot be read is a NULL cell, never a failed row: the
/// filesystem is allowed to be messy and a query over it should still return.
/// `size` is what the row's stat reported, so the buffer is sized without
/// stat'ing again; a file that has grown since is still read whole.
fn read_text(path: &Path, size: Option<i64>) -> Option<String> {
    read_text_with(path, size, |path| File::open(path))
}

fn read_text_with<R: Read>(
    path: &Path,
    size: Option<i64>,
    open: impl FnOnce(&Path) -> io::Result<R>,
) -> Option<String> {
    let mut file = open(path).ok()?;
    let bytes = read_all(&mut file, presize(size)).ok()?;
    String::from_utf8(bytes).ok()
}

/// One past the size the stat saw: a file unchanged since fills the buffer in
/// one read and reports its end on the next, with no stat in between.
fn presize(size: Option<i64>) -> usize {
    usize::try_from(size.unwrap_or(0))
        .unwrap_or(0)
        .saturating_add(1)
}

/// Every byte `reader` yields, into a buffer of `expected` bytes that grows
/// if the reader has more.
fn read_all(reader: &mut impl Read, expected: usize) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve_exact(expected.max(1))?;
    buf.resize(expected.max(1), 0);
    let mut filled = 0;
    loop {
        if filled == buf.len() {
            buf.resize(buf.len() * 2, 0);
        }
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Read `content` for every row in `rows` that has none yet, [`READERS`]
/// files at a time, each reader taking the next unread row. `read` is
/// injected so the sharing-out is testable without a filesystem.
fn read_contents(rows: &[&FileRow], read: &(dyn Fn(&FileRow) -> Option<String> + Sync)) {
    let unread: Vec<&FileRow> = rows
        .iter()
        .copied()
        .filter(|row| row.content.get().is_none())
        .collect();
    share_out(&unread, READERS, &|row| {
        row.content.get_or_init(|| Box::new(read(row)));
    });
}

/// Stat every row in `rows` that has no facts yet, shared out across every
/// core: each stat is a syscall the kernel answers independently of the
/// last. `stat` is injected so the sharing-out is testable without a
/// filesystem.
fn stat_rows(rows: &[&FileRow], stat: &(dyn Fn(&FileRow) -> StatFacts + Sync)) {
    let unstatted: Vec<&FileRow> = rows
        .iter()
        .copied()
        .filter(|row| row.facts.get().is_none())
        .collect();
    let workers = thread::available_parallelism().map_or(1, usize::from);
    share_out(&unstatted, workers, &|row| {
        row.facts.get_or_init(|| Box::new(stat(row)));
    });
}

/// Run `work` on every row, `workers` at a time, each worker taking the next
/// row not yet taken.
fn share_out(rows: &[&FileRow], workers: usize, work: &(dyn Fn(&FileRow) + Sync)) {
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..workers.min(rows.len()) {
            scope.spawn(|| {
                while let Some(row) = rows.get(next.fetch_add(1, Ordering::Relaxed)) {
                    work(row);
                }
            });
        }
    });
}

/// Everything a scan needs: where to walk, what to match, what to call the
/// results, and what to skip.
struct ScanSpec {
    root: PathBuf,
    glob: PathGlob,
    /// Prepended to each matched path before the stat columns are computed.
    /// Empty for index-root-relative tables.
    path_prefix: PathBuf,
    ignore: TableMatcher,
    /// Whether the scan respects `.gitignore` files (off under `--no-ignore`).
    gitignore: bool,
    /// Whether a `.gitignore` that ignores `root` empties the scan.
    check_root: bool,
    reader: fn(&Path, Option<i64>) -> Option<String>,
    /// The row set of the statement in progress, which the first stat a
    /// statement asks for stats whole: one stat at a time as SQLite steps
    /// would be several times slower than sharing them out at once.
    current: Mutex<Weak<RowStream<FileRow>>>,
}

/// One matched file, as compact as the seven stat columns allow: the three
/// path-derived text columns are slices of the path located by
/// [`PathSpans`], so a row costs two allocations however many columns a query
/// reads.
struct FileRow {
    /// The path as scanned, root-relative, when the reported path is not
    /// already it.
    rel: Option<Box<Path>>,
    /// The path as reported, under the table's prefix when it has one, then
    /// a NUL: SQLite reads a NUL-terminated cell in place, and `basename`
    /// and `ext` are suffixes that end on the same NUL.
    terminated: Box<str>,
    spans: PathSpans,
    /// Stat'ed at most once per statement, and only when the statement reads
    /// a stat column or the content.
    facts: OnceLock<Box<StatFacts>>,
    /// The file's text once a statement has asked for it, read at most once
    /// per statement: ahead for every row when the statement names the
    /// column, or on demand for the row being emitted.
    content: OnceLock<Box<Option<String>>>,
}

impl FileRow {
    fn new(path_prefix: &Path, rel: PathBuf) -> Self {
        let (rel, path) = match reported_in_place(path_prefix, rel) {
            Ok(path) => (None, path),
            Err(rel) => {
                let path = reported_path(path_prefix, &rel);
                (Some(rel.into_boxed_path()), path)
            }
        };
        let spans = PathSpans::of(&path);
        let mut terminated = path;
        terminated.push('\0');
        Self {
            rel,
            terminated: terminated.into_boxed_str(),
            spans,
            facts: OnceLock::new(),
            content: OnceLock::new(),
        }
    }

    /// The path as scanned, root-relative; where the lazy content read goes.
    fn rel(&self) -> &Path {
        self.rel
            .as_deref()
            .unwrap_or_else(|| Path::new(self.path()))
    }

    fn path(&self) -> &str {
        &self.terminated[..self.terminated.len() - 1]
    }

    /// Column `i` with its terminating NUL, for the columns that end the
    /// path; `None` for any other column, or one the path has no answer for.
    fn terminated_cell(&self, i: c_int) -> Option<&str> {
        match usize::try_from(i).ok()? {
            PATH_COLUMN => Some(&self.terminated),
            BASENAME_COLUMN => self
                .spans
                .name_at
                .map(|at| &self.terminated[span_end(at)..]),
            EXT_COLUMN => self.spans.ext_at.map(|at| &self.terminated[span_end(at)..]),
            _ => None,
        }
    }

    fn basename(&self) -> Option<&str> {
        self.spans.name_at.map(|at| &self.path()[span_end(at)..])
    }

    fn dir(&self) -> Option<&str> {
        self.spans
            .dir_len
            .map(|len| &self.terminated[..span_end(len)])
    }

    fn ext(&self) -> Option<&str> {
        self.spans.ext_at.map(|at| &self.path()[span_end(at)..])
    }
}

/// Where `basename`, `dir` and `ext` sit inside a reported path, each `None`
/// where [`Path`] has no answer: no basename or dir for an empty path, no ext
/// for a dotfile or an extensionless name. [`Path::parent`] is a prefix of
/// the path and [`Path::file_name`] and [`Path::extension`] are suffixes, so
/// one length each locates them. Each is held one past its offset, so an
/// absent span costs no more room than a present one.
#[derive(Debug, PartialEq, Eq)]
struct PathSpans {
    name_at: Option<NonZeroU32>,
    dir_len: Option<NonZeroU32>,
    ext_at: Option<NonZeroU32>,
}

/// The offset a [`PathSpans`] span holds one past.
fn span_end(span: NonZeroU32) -> usize {
    (span.get() - 1) as usize
}

impl PathSpans {
    fn of(path: &str) -> Self {
        let p = Path::new(path);
        let span = |at: usize| {
            u32::try_from(at)
                .ok()?
                .checked_add(1)
                .and_then(NonZeroU32::new)
        };
        let suffix_at = |s: &std::ffi::OsStr| span(path.len() - s.len());
        Self {
            name_at: p.file_name().and_then(suffix_at),
            dir_len: p.parent().and_then(|d| span(d.as_os_str().len())),
            ext_at: p.extension().and_then(suffix_at),
        }
    }
}

/// The stat-derived columns, each `None` when the file could not be stat'ed
/// or the platform cannot supply the fact (or it predates the epoch).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatFacts {
    size: Option<i64>,
    mtime: Option<i64>,
    ctime: Option<i64>,
}

impl StatFacts {
    fn from_parts(len: u64, modified: Option<SystemTime>, created: Option<SystemTime>) -> Self {
        Self {
            size: i64::try_from(len).ok(),
            mtime: epoch_secs(modified),
            ctime: epoch_secs(created),
        }
    }
}

fn epoch_secs(time: Option<SystemTime>) -> Option<i64> {
    let since = time?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(since.as_secs()).ok()
}

fn integer(n: Option<i64>) -> ValueRef<'static> {
    n.map_or(ValueRef::Null, ValueRef::Integer)
}

/// The cell for column `i` of `row`, its stat columns from `facts`; `None`
/// for `content`, which is read from the file rather than stat'ed.
fn cell<'r>(
    row: &'r FileRow,
    i: c_int,
    facts: &dyn Fn(&FileRow) -> StatFacts,
) -> Option<ValueRef<'r>> {
    Some(match usize::try_from(i).unwrap_or(usize::MAX) {
        PATH_COLUMN => ValueRef::from(row.path()),
        BASENAME_COLUMN => ValueRef::from(row.basename()),
        DIR_COLUMN => ValueRef::from(row.dir()),
        EXT_COLUMN => ValueRef::from(row.ext()),
        SIZE_COLUMN => integer(facts(row).size),
        MTIME_COLUMN => integer(facts(row).mtime),
        CTIME_COLUMN => integer(facts(row).ctime),
        CONTENT_COLUMN => return None,
        _ => ValueRef::Null,
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
        gitignore: vtab_scaffold::parse_gitignore(gitignore)?,
        check_root: vtab_scaffold::checks_root(gitignore),
        reader: read_text,
        current: Mutex::new(Weak::new()),
    })
}

impl ScanSpec {
    fn stat(&self, row: &FileRow) -> StatFacts {
        fs::metadata(self.root.join(row.rel())).map_or_else(
            |_| StatFacts::default(),
            |m| StatFacts::from_parts(m.len(), m.modified().ok(), m.created().ok()),
        )
    }

    fn facts(&self, row: &FileRow) -> StatFacts {
        if let Some(facts) = row.facts.get() {
            return **facts;
        }
        let current = self
            .current
            .lock()
            .map_or_else(|_| Weak::new(), |c| c.clone());
        if let Some(rows) = current.upgrade() {
            stat_rows(&rows.all().iter().collect::<Vec<_>>(), &|row| {
                self.stat(row)
            });
        }
        **row.facts.get_or_init(|| Box::new(self.stat(row)))
    }

    fn read(&self, row: &FileRow) -> Option<String> {
        (self.reader)(&self.root.join(row.rel()), self.facts(row).size)
    }
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

/// The reported path taken from `rel` itself, when it is already what a
/// table with no prefix reports: handing `rel` back otherwise.
fn reported_in_place(path_prefix: &Path, rel: PathBuf) -> Result<String, PathBuf> {
    if !path_prefix.as_os_str().is_empty() || std::path::MAIN_SEPARATOR != '/' {
        return Err(rel);
    }
    rel.into_os_string().into_string().map_err(PathBuf::from)
}

impl TableSource for ScanSpec {
    type Row = FileRow;

    const NAME: &'static str = MODULE_NAME;

    const LOOKUP_COLUMNS: &'static [usize] = &LOOKUP_COLUMNS;

    fn connect(args: &[&[u8]]) -> Result<(String, Self)> {
        Ok((declared_schema(), parse_module_args(args)?))
    }

    fn rows(&self, rows: &Arc<RowStream<FileRow>>) {
        if let Ok(mut current) = self.current.lock() {
            *current = Arc::downgrade(rows);
        }
        // The scan runs per statement rather than at CREATE, which is what
        // makes reads live: each statement sees the filesystem as it is now.
        let mut chunk = Vec::with_capacity(STREAM_CHUNK_ROWS);
        scan_glob_rows(
            &self.root,
            &self.glob,
            &self.ignore,
            self.gitignore,
            self.check_root,
            &|rel| FileRow::new(&self.path_prefix, rel),
            &mut |row| {
                chunk.push(row);
                if chunk.len() == STREAM_CHUNK_ROWS {
                    let full = std::mem::replace(&mut chunk, Vec::with_capacity(STREAM_CHUNK_ROWS));
                    rows.push(Arc::new(full));
                }
            },
        );
        rows.push(Arc::new(chunk));
    }

    fn column(&self, row: &FileRow, ctx: &mut Context, i: c_int) -> Result<()> {
        if let Some(text) = row.terminated_cell(i) {
            set_terminated_text(ctx, text);
            return Ok(());
        }
        match cell(row, i, &|row| self.facts(row)) {
            Some(value) => ctx.set_result(&ToSqlOutput::Borrowed(value)),
            // The one effectful read, reached only when a query names the
            // column: this is where laziness actually lives.
            None => ctx.set_result(&**row.content.get_or_init(|| Box::new(self.read(row)))),
        }
    }

    const PREFETCH_COLUMN: Option<usize> = Some(CONTENT_COLUMN);

    fn prefetch(&self, rows: &[&FileRow]) {
        read_contents(rows, &|row| self.read(row));
    }

    fn lookup_key<'r>(&self, row: &'r FileRow, column: usize) -> Option<&'r str> {
        match column {
            PATH_COLUMN => Some(row.path()),
            BASENAME_COLUMN => row.basename(),
            DIR_COLUMN => row.dir(),
            _ => None,
        }
    }
}

/// Hand SQLite `text`, which ends in a NUL, without copying it. rusqlite
/// only offers `SQLITE_TRANSIENT`, which copies every cell.
#[expect(
    unsafe_code,
    reason = "rusqlite exposes neither the raw context nor SQLITE_STATIC"
)]
fn set_terminated_text(ctx: &mut Context, text: &str) {
    const _: () = assert!(size_of::<Context>() == size_of::<*mut ffi::sqlite3_context>());
    // SAFETY: `Context` is a newtype over the raw context pointer, as the
    // size assert above checks. SQLITE_STATIC is sound because the rows stay
    // held by the statement scope until the statement is finalized. A length
    // of -1 marks the value NUL-terminated, so SQLite never copies it to add one.
    unsafe {
        let raw = *std::ptr::from_mut(ctx).cast::<*mut ffi::sqlite3_context>();
        ffi::sqlite3_result_text(raw, text.as_ptr().cast(), -1, ffi::SQLITE_STATIC());
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
        let args = args_with(&[b"'/tmp'", b"'[z-a]'", b"''", b"'gitignore'"]);
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
        FileRow::new(Path::new(prefix), PathBuf::from(rel))
    }

    #[cfg(unix)]
    #[test]
    fn a_row_without_a_prefix_keeps_the_scanned_path_allocation() {
        let mut rel = PathBuf::with_capacity("docs/a.md".len() + 1);
        rel.push("docs/a.md");
        let scanned = rel.as_os_str().as_encoded_bytes().as_ptr();

        let row = FileRow::new(Path::new(""), rel);

        assert_eq!(row.terminated.as_ptr(), scanned);
    }

    #[test]
    fn the_cells_that_end_the_path_keep_its_terminator() {
        let row = row_for("", "docs/a.md");
        let cells: Vec<Option<&str>> = (0..=8).map(|i| row.terminated_cell(i)).collect();
        assert_eq!(
            cells,
            vec![
                Some("docs/a.md\0"),
                Some("a.md\0"),
                None,
                Some("md\0"),
                None,
                None,
                None,
                None,
                None
            ]
        );
        assert_eq!(row.terminated_cell(-1), None);
    }

    #[test]
    fn a_path_without_an_ext_has_no_terminated_ext() {
        assert_eq!(row_for("", "Makefile").terminated_cell(3), None);
    }

    fn no_facts(_: &FileRow) -> StatFacts {
        StatFacts::default()
    }

    fn injected_reader(path: &Path, size: Option<i64>) -> Option<String> {
        (path == Path::new("/root/docs/a.md") && size == Some(7)).then(|| "contents".to_owned())
    }

    fn sized_row(rel: &str, size: i64) -> FileRow {
        let row = FileRow::new(Path::new(""), PathBuf::from(rel));
        row.facts.set(Box::new(facts(size))).unwrap();
        row
    }

    fn spec_with_reader() -> ScanSpec {
        let args = args_with(&[b"'/root'", b"'**/*.md'", b"''", b"'gitignore'"]);
        let mut spec = parse_module_args(&args).unwrap();
        spec.reader = injected_reader;
        spec
    }

    #[test]
    fn read_text_uses_the_injected_reader_and_decodes_utf8() {
        let text = read_text_with(Path::new("/root/docs/a.md"), Some(7), |path| {
            assert_eq!(path, Path::new("/root/docs/a.md"));
            Ok(std::io::Cursor::new(b"contents".to_vec()))
        });

        assert_eq!(text.as_deref(), Some("contents"));
    }

    #[test]
    fn read_text_reads_a_real_file() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");

        let text = read_text(&path, None).unwrap();

        assert!(text.starts_with("[package]"));
        assert!(text.contains("name = \"dirsql\""));
    }

    #[test]
    fn scan_spec_read_uses_the_injected_reader() {
        let spec = spec_with_reader();
        let row = sized_row("docs/a.md", 7);

        assert_eq!(spec.read(&row).as_deref(), Some("contents"));
    }

    #[test]
    fn prefetch_caches_content_from_the_injected_reader() {
        let spec = spec_with_reader();
        let row = sized_row("docs/a.md", 7);
        let rows = [&row];

        spec.prefetch(&rows);

        assert_eq!(
            row.content.get().map(|c| &**c),
            Some(&Some("contents".to_owned()))
        );
    }

    fn facts(size: i64) -> StatFacts {
        StatFacts {
            size: Some(size),
            mtime: Some(size * 10),
            ctime: Some(size * 100),
        }
    }

    fn text(s: &str) -> Option<ValueRef<'_>> {
        Some(ValueRef::Text(s.as_bytes()))
    }

    #[test]
    fn each_stat_column_reads_its_own_cell() {
        let row = row_for("", "docs/a.md");
        let seven = |_: &FileRow| facts(7);
        assert_eq!(cell(&row, 0, &seven), text("docs/a.md"));
        assert_eq!(cell(&row, 1, &seven), text("a.md"));
        assert_eq!(cell(&row, 2, &seven), text("docs"));
        assert_eq!(cell(&row, 3, &seven), text("md"));
        assert_eq!(cell(&row, 4, &seven), Some(ValueRef::Integer(7)));
        assert_eq!(cell(&row, 5, &seven), Some(ValueRef::Integer(70)));
        assert_eq!(cell(&row, 6, &seven), Some(ValueRef::Integer(700)));
    }

    #[test]
    fn a_path_column_stats_nothing() {
        let row = row_for("", "docs/a.md");
        let stat = |_: &FileRow| panic!("a path column needs no stat");
        for i in [0, 1, 2, 3, 7, 8] {
            cell(&row, i, &stat);
        }
    }

    #[test]
    fn the_content_column_is_not_a_stored_cell() {
        let row = row_for("", "a.md");
        assert_eq!(cell(&row, 7, &no_facts), None);
    }

    #[test]
    fn an_absent_fact_is_a_null_cell() {
        let row = row_for("", "Makefile");
        assert_eq!(cell(&row, 3, &no_facts), Some(ValueRef::Null), "no ext");
        assert_eq!(cell(&row, 4, &no_facts), Some(ValueRef::Null), "no size");
    }

    #[test]
    fn a_column_past_the_schema_is_null() {
        let row = row_for("", "a.md");
        assert_eq!(cell(&row, 8, &no_facts), Some(ValueRef::Null));
        assert_eq!(cell(&row, -1, &no_facts), Some(ValueRef::Null));
    }

    #[test]
    fn lookup_columns_are_path_basename_and_dir_in_that_order() {
        let names: Vec<&str> = LOOKUP_COLUMNS.iter().map(|c| STAT_COLUMNS[*c]).collect();
        assert_eq!(names, ["path", "basename", "dir"]);
    }

    #[test]
    fn a_scan_spec_keys_a_row_by_the_text_of_the_lookup_column() {
        let args = args_with(&[b"'/tmp/notes'", b"'**/*.md'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();
        let row = row_for("", "docs/a.md");

        assert_eq!(spec.lookup_key(&row, 0), Some("docs/a.md"));
        assert_eq!(spec.lookup_key(&row, 1), Some("a.md"));
        assert_eq!(spec.lookup_key(&row, 2), Some("docs"));
        assert_eq!(
            spec.lookup_key(&row, 4),
            None,
            "size is not a lookup column"
        );
    }

    #[test]
    fn a_row_reports_its_path_under_the_prefix_and_reads_from_the_scanned_one() {
        let row = FileRow::new(Path::new("/root"), PathBuf::from("f001.md"));
        assert_eq!(row.path(), "/root/f001.md");
        assert_eq!(row.rel(), Path::new("f001.md"));
    }

    #[test]
    fn a_new_row_is_unstatted_and_unread() {
        let row = FileRow::new(Path::new(""), PathBuf::from("a.md"));
        assert!(row.facts.get().is_none());
        assert!(row.content.get().is_none());
    }

    #[test]
    fn the_first_stat_a_statement_asks_for_stats_its_whole_row_set() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), "aa").unwrap();
        fs::write(dir.path().join("b.md"), "bbb").unwrap();
        let root = dir.path().to_str().unwrap().as_bytes();
        let args = args_with(&[root, b"'*.md'", b"''", b"'no-gitignore'"]);
        let spec = parse_module_args(&args).unwrap();
        let rows = scanned(&spec);
        let rows = rows.all();
        assert!(rows.iter().all(|row| row.facts.get().is_none()));

        assert_eq!(spec.facts(rows.get(0).unwrap()).size, Some(2));

        assert_eq!(
            rows.get(1).unwrap().facts.get().and_then(|f| f.size),
            Some(3)
        );
    }

    fn scanned(spec: &ScanSpec) -> Arc<RowStream<FileRow>> {
        let rows = Arc::new(RowStream::default());
        spec.rows(&rows);
        rows.finish(false);
        rows
    }

    #[test]
    fn a_scan_hands_its_rows_over_in_full_chunks_and_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let names: Vec<String> = (0..=STREAM_CHUNK_ROWS)
            .map(|n| format!("f{n:05}.md"))
            .collect();
        for name in &names {
            fs::write(dir.path().join(name), "").unwrap();
        }
        let root = dir.path().to_str().unwrap().as_bytes();
        let args = args_with(&[root, b"'*.md'", b"''", b"'no-gitignore'"]);
        let rows = scanned(&parse_module_args(&args).unwrap());

        assert_eq!(rows.chunk(0).unwrap().len(), STREAM_CHUNK_ROWS);
        assert_eq!(rows.chunk(1).unwrap().len(), 1);
        assert!(rows.chunk(2).is_none());
        let all = rows.all();
        let paths: Vec<&str> = all.iter().map(FileRow::path).collect();
        assert_eq!(paths, names);
    }

    #[test]
    fn stat_rows_stats_each_unstatted_row_once() {
        let rows: Vec<FileRow> = (0..40).map(|n| row_for("", &format!("f{n}.md"))).collect();
        rows[3].facts.set(Box::new(facts(1))).unwrap();
        let stats = AtomicUsize::new(0);
        let stat = |row: &FileRow| {
            stats.fetch_add(1, Ordering::Relaxed);
            facts(row.path()[1..row.path().len() - 3].parse().unwrap())
        };

        stat_rows(&rows.iter().collect::<Vec<_>>(), &stat);

        assert_eq!(stats.load(Ordering::Relaxed), 39);
        assert_eq!(rows[3].facts.get().map(|f| **f), Some(facts(1)));
        assert_eq!(rows[7].facts.get().map(|f| **f), Some(facts(7)));
        stat_rows(&[], &|_| panic!("nothing to stat"));
    }

    #[test]
    fn presize_is_one_past_the_size_the_scan_saw() {
        assert_eq!(presize(Some(0)), 1);
        assert_eq!(presize(Some(41)), 42);
    }

    #[test]
    fn presize_is_one_byte_without_a_usable_size() {
        assert_eq!(presize(None), 1);
        assert_eq!(presize(Some(-1)), 1);
    }

    /// A reader that yields `chunks` one call at a time, `interrupted` times
    /// interrupted first.
    struct Chunked {
        chunks: Vec<Vec<u8>>,
        interrupted: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.interrupted > 0 {
                self.interrupted -= 1;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            if self.chunks.is_empty() {
                return Ok(0);
            }
            let chunk = self.chunks.remove(0);
            let n = chunk.len().min(buf.len());
            buf[..n].copy_from_slice(&chunk[..n]);
            if n < chunk.len() {
                self.chunks.insert(0, chunk[n..].to_vec());
            }
            Ok(n)
        }
    }

    #[test]
    fn read_all_takes_every_byte_a_presized_read_yields() {
        let mut reader = Chunked {
            chunks: vec![b"hello".to_vec()],
            interrupted: 0,
        };
        assert_eq!(read_all(&mut reader, 6).unwrap(), b"hello");
    }

    #[test]
    fn read_all_grows_past_a_size_the_file_has_outgrown() {
        let mut reader = Chunked {
            chunks: vec![b"hello".to_vec(), b" world".to_vec()],
            interrupted: 0,
        };
        assert_eq!(read_all(&mut reader, 2).unwrap(), b"hello world");
        let mut reader = Chunked {
            chunks: vec![b"hello".to_vec()],
            interrupted: 0,
        };
        assert_eq!(read_all(&mut reader, 0).unwrap(), b"hello");
    }

    struct Counting<R>(R, usize);

    impl<R: Read> Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.1 += 1;
            self.0.read(buf)
        }
    }

    #[test]
    fn read_all_grows_geometrically_when_a_file_has_outgrown_its_size() {
        let chunked = Chunked {
            chunks: vec![vec![b'x'; 4096]],
            interrupted: 0,
        };
        let mut reader = Counting(chunked, 0);
        assert_eq!(read_all(&mut reader, 1).unwrap().len(), 4096);
        assert!(reader.1 <= 16, "{} reads", reader.1);
    }

    #[test]
    fn read_all_retries_an_interrupted_read() {
        let mut reader = Chunked {
            chunks: vec![b"hi".to_vec()],
            interrupted: 2,
        };
        assert_eq!(read_all(&mut reader, 3).unwrap(), b"hi");
    }

    #[test]
    fn read_all_surfaces_a_failed_read() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            }
        }
        let err = read_all(&mut Broken, 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn read_all_yields_nothing_for_an_empty_reader() {
        let mut reader = Chunked {
            chunks: Vec::new(),
            interrupted: 0,
        };
        assert_eq!(read_all(&mut reader, 1).unwrap(), b"");
    }

    #[test]
    fn read_contents_reads_each_unread_row_once() {
        let rows: Vec<FileRow> = (0..40).map(|n| row_for("", &format!("f{n}.md"))).collect();
        rows[3]
            .content
            .set(Box::new(Some("already".to_string())))
            .unwrap();
        let reads = std::sync::Mutex::new(Vec::new());
        let read = |row: &FileRow| {
            reads.lock().unwrap().push(row.path().to_string());
            Some(format!("body of {}", row.path()))
        };

        read_contents(&rows.iter().collect::<Vec<_>>(), &read);

        let mut reads = reads.into_inner().unwrap();
        reads.sort();
        let mut expected: Vec<String> = (0..40)
            .filter(|&n| n != 3)
            .map(|n| format!("f{n}.md"))
            .collect();
        expected.sort();
        assert_eq!(
            reads, expected,
            "every row but the one already read, once each"
        );
        assert_eq!(rows[3].content.get().unwrap().as_deref(), Some("already"));
        assert_eq!(
            rows[7].content.get().unwrap().as_deref(),
            Some("body of f7.md")
        );
    }

    #[test]
    fn read_contents_keeps_a_null_read() {
        let rows = [row_for("", "a.md")];
        read_contents(&[&rows[0]], &|_| None);
        assert_eq!(rows[0].content.get().map(|c| &**c), Some(&None));
    }

    #[test]
    fn read_contents_reads_nothing_when_every_row_is_read() {
        let rows = [row_for("", "a.md")];
        rows[0].content.set(Box::new(None)).unwrap();
        read_contents(&[&rows[0]], &|_| {
            panic!("a row read already is not read again")
        });
        read_contents(&[], &|_| panic!("nothing to read"));
    }

    #[test]
    fn a_nested_path_splits_into_dir_basename_and_ext() {
        let row = row_for("", "nested/sub.txt");
        assert_eq!(row.path(), "nested/sub.txt");
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
        assert_eq!(row.path(), "/var/log/a.log");
        assert_eq!(row.dir(), Some("/var/log"));
        assert_eq!(row.basename(), Some("a.log"));
        assert_eq!(
            row.rel(),
            Path::new("a.log"),
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
        let args = args_with(&[b"'/nonexistent-dirsql-root'", b"'*'", b"''", b"'gitignore'"]);
        let spec = parse_module_args(&args).unwrap();
        assert_eq!(row.basename(), Some("bare"));
        assert_eq!(spec.facts(&row), StatFacts::default());
    }

    #[test]
    fn stat_facts_come_from_the_length_and_the_two_times() {
        let t = UNIX_EPOCH + std::time::Duration::from_secs(5);
        assert_eq!(
            StatFacts::from_parts(3, Some(t), None),
            StatFacts {
                size: Some(3),
                mtime: Some(5),
                ctime: None,
            }
        );
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

    #[test]
    fn a_row_not_yet_stat_or_read_stays_compact() {
        let size = std::mem::size_of::<FileRow>();
        assert!(size <= 80, "a FileRow is {size} bytes");
    }
    #[test]
    fn a_content_column_reads_the_file_through_the_module() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.md"), "hi").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        load_module(&conn, StatementScope::new()).unwrap();
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE t USING dirsql_path('{}', '*.md', '', 'no-gitignore')",
            dir.path().display()
        ))
        .unwrap();

        let content: String = conn
            .query_row("SELECT content FROM t", [], |row| row.get(0))
            .unwrap();

        assert_eq!(content, "hi");
    }
}
