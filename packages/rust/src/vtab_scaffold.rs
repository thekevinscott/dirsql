//! The scaffolding both `dirsql_*` virtual tables are built from: decoding the
//! arguments SQLite hands `CREATE VIRTUAL TABLE`, and the read-only table and
//! cursor those arguments drive.
//!
//! A table implements [`TableSource`] and keeps only what actually differs
//! between the two — which module arguments it takes, how it produces a row
//! set, and how a row becomes a cell. Everything either one would otherwise
//! write twice lives here.

use std::ffi::c_int;
use std::sync::{Arc, Mutex, Weak};

use globset::GlobSet;
use rusqlite::vtab::{
    Context, CreateVTab, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind, Values,
    read_only_module,
};
use rusqlite::{Connection, Error, Result, ffi};

use crate::Value;
use crate::matcher::TableMatcher;
use crate::scanner;
use crate::sql_literal::unquote;

/// Cost a full scan is declared to carry. Both tables answer `xBestIndex` the
/// same way: there is no index to choose, so the only job is to stop SQLite
/// assuming the near-infinite default and reordering a join around it.
const SCAN_COST: f64 = 1000.;

/// What a `dirsql_*` table supplies on top of the scaffolding.
pub trait TableSource: Sized + 'static {
    /// One row, as the table models it.
    type Row: Send + Sync + 'static;

    /// SQL module name the table is created with.
    const NAME: &'static str;

    /// Decode the module arguments into the table's source and the schema it
    /// declares to SQLite.
    fn connect(args: &[&[u8]]) -> Result<(String, Self)>;

    /// The row set one statement reads, taken on the statement's first
    /// `xFilter` over the table and held for the rest of it. A table whose
    /// reads are live rescans here; one that materialized at `CREATE` hands
    /// back the `Arc` it already holds.
    fn rows(&self) -> Arc<Vec<Self::Row>>;

    /// Emit column `i` of `row`.
    fn column(&self, row: &Self::Row, ctx: &mut Context, i: c_int) -> Result<()>;
}

/// The row sets the tables on one connection hold across a statement.
///
/// SQLite opens a fresh cursor for every reference to a table — each CTE,
/// each `UNION` arm, each pass of a join — and a path table that rescanned on
/// every cursor would walk the tree once per reference. Instead a table keeps
/// the rows its first cursor in a statement produced and serves the same rows
/// to the rest. What ends a statement is [`reset`](Self::reset): the owner of
/// the connection brackets each statement with it, so the next statement
/// scans afresh and the rows do not outlive the statement that read them.
#[derive(Default)]
pub struct StatementScope {
    caches: Mutex<Vec<Weak<dyn Evict + Send + Sync>>>,
}

impl StatementScope {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Reset now and again when the returned guard drops: the two edges of
    /// one statement.
    pub fn enter(self: &Arc<Self>) -> StatementGuard {
        self.reset();
        StatementGuard(Arc::clone(self))
    }

    /// Drop every row set a table under this scope is holding, so its next
    /// read scans afresh.
    pub fn reset(&self) {
        let mut caches = self.caches.lock().unwrap();
        caches.retain(|cache| {
            cache.upgrade().is_some_and(|cache| {
                cache.evict();
                true
            })
        });
    }

    fn adopt(&self, cache: Weak<dyn Evict + Send + Sync>) {
        self.caches.lock().unwrap().push(cache);
    }
}

pub struct StatementGuard(Arc<StatementScope>);

impl Drop for StatementGuard {
    fn drop(&mut self) {
        self.0.reset();
    }
}

trait Evict {
    fn evict(&self);
}

/// One table's rows for the statement in progress, empty between statements.
struct RowCache<R> {
    rows: Mutex<Option<Arc<Vec<R>>>>,
}

impl<R> Default for RowCache<R> {
    fn default() -> Self {
        Self {
            rows: Mutex::new(None),
        }
    }
}

impl<R> RowCache<R> {
    /// The rows already held, or the ones `scan` produces, held from now on.
    fn rows_or_else(&self, scan: impl FnOnce() -> Arc<Vec<R>>) -> Arc<Vec<R>> {
        let mut slot = self.rows.lock().unwrap();
        match &*slot {
            Some(rows) => Arc::clone(rows),
            None => {
                let rows = scan();
                *slot = Some(Arc::clone(&rows));
                rows
            }
        }
    }
}

impl<R> Evict for RowCache<R> {
    fn evict(&self) {
        *self.rows.lock().unwrap() = None;
    }
}

/// Register `S` as a virtual-table module on `conn`, its tables caching rows
/// under `scope`.
pub fn load_module<S: TableSource>(conn: &Connection, scope: Arc<StatementScope>) -> Result<()> {
    conn.create_module(S::NAME, read_only_module::<ScaffoldTab<S>>(), Some(scope))
}

/// The module's own arguments: the three SQLite prepends (module, database and
/// table name) dropped, and every SQL literal unquoted.
pub fn user_args(args: &[&[u8]]) -> Vec<String> {
    args.iter()
        .skip(3)
        .map(|a| unquote(&String::from_utf8_lossy(a)).to_string())
        .collect()
}

/// The error a module raises when it was handed too few arguments. Named
/// arity plus the argument list, so SQLite reports what to write against the
/// offending `CREATE VIRTUAL TABLE`.
pub fn arity_error(module: &str, wanted: usize, names: &str, got: usize) -> Error {
    Error::ModuleError(format!(
        "{module} takes at least {wanted} arguments ({names}), got {got}"
    ))
}

fn no_scope_error(module: &str) -> Error {
    Error::ModuleError(format!(
        "{module} was registered without a statement scope; load it through dirsql::vtab::load_module"
    ))
}

/// Compile a table's glob, surfacing a bad pattern as a module error so SQLite
/// reports it against the `CREATE VIRTUAL TABLE` statement.
pub fn compile_glob(pattern: &str) -> Result<GlobSet> {
    scanner::compile_glob(pattern).map_err(|e| Error::ModuleError(e.to_string()))
}

/// Compile the ignore patterns a scan applies.
pub fn compile_ignore(patterns: &[String]) -> Result<TableMatcher> {
    let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
    TableMatcher::new(&[], &refs).map_err(|e| Error::ModuleError(e.to_string()))
}

/// Whether the scan respects `.gitignore` files, from the module's switch
/// argument.
pub fn parse_gitignore(arg: &str) -> Result<bool> {
    scanner::parse_gitignore_arg(arg).map_err(Error::ModuleError)
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
pub struct ScaffoldTab<S: TableSource> {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
    source: Arc<S>,
    cache: Arc<RowCache<S::Row>>,
}

#[expect(unsafe_code, reason = "rusqlite requires an unsafe trait impl")]
unsafe impl<'vtab, S: TableSource> VTab<'vtab> for ScaffoldTab<S> {
    type Aux = Arc<StatementScope>;
    type Cursor = ScaffoldCursor<S>;

    fn connect(
        _db: &mut VTabConnection,
        aux: Option<&Arc<StatementScope>>,
        args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let scope = aux.ok_or_else(|| no_scope_error(S::NAME))?;
        let (schema, source) = S::connect(args)?;
        let cache = Arc::new(RowCache::default());
        scope.adopt(Arc::downgrade(&cache) as Weak<dyn Evict + Send + Sync>);
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
            source: Arc::new(source),
            cache,
        };
        Ok((schema, vtab))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        info.set_estimated_cost(SCAN_COST);
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<ScaffoldCursor<S>> {
        Ok(ScaffoldCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            source: Arc::clone(&self.source),
            cache: Arc::clone(&self.cache),
            rows: Arc::new(Vec::new()),
            index: 0,
        })
    }
}

impl<'vtab, S: TableSource> CreateVTab<'vtab> for ScaffoldTab<S> {
    const KIND: VTabKind = VTabKind::Default;
}

#[repr(C)]
pub struct ScaffoldCursor<S: TableSource> {
    /// Base class. Must be first: `rust_open` hands this pointer straight to
    /// SQLite as a `sqlite3_vtab_cursor`, so anything ahead of it gets
    /// overwritten.
    base: ffi::sqlite3_vtab_cursor,
    source: Arc<S>,
    cache: Arc<RowCache<S::Row>>,
    rows: Arc<Vec<S::Row>>,
    index: usize,
}

#[expect(unsafe_code, reason = "rusqlite requires an unsafe trait impl")]
unsafe impl<S: TableSource> VTabCursor for ScaffoldCursor<S> {
    fn filter(
        &mut self,
        _idx_num: c_int,
        _idx_str: Option<&str>,
        _args: &Values<'_>,
    ) -> Result<()> {
        self.rows = self.cache.rows_or_else(|| self.source.rows());
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
        match self.rows.get(self.index) {
            Some(row) => self.source.column(row, ctx, i),
            None => ctx.set_result(&Value::Null),
        }
    }

    fn rowid(&self) -> Result<i64> {
        Ok(rowid_of(self.index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // The `VTab`/`VTabCursor` impls need a real `rusqlite::Connection`, which
    // `unit lint`'s isolation rule forbids here; they are covered over real
    // directories by `tests/vtab.rs` and `tests/schema_inference.rs`.

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
    fn user_args_drops_the_three_sqlite_prepends() {
        let args = args_with(&[b"'/tmp'", b"'**/*'"]);
        assert_eq!(
            user_args(&args),
            vec!["/tmp".to_string(), "**/*".to_string()]
        );
    }

    #[test]
    fn user_args_unquotes_each_sql_literal() {
        let args = args_with(&[b"'a''b'"]);
        assert_eq!(user_args(&args), vec!["a'b".to_string()]);
    }

    #[test]
    fn user_args_accepts_unquoted_arguments() {
        let args = args_with(&[b"/tmp/notes"]);
        assert_eq!(user_args(&args), vec!["/tmp/notes".to_string()]);
    }

    #[test]
    fn user_args_is_empty_when_the_module_took_none() {
        assert_eq!(user_args(&args_with(&[])), Vec::<String>::new());
    }

    #[test]
    fn arity_error_names_the_module_the_arity_and_the_arguments() {
        let err = arity_error("dirsql_path", 4, "root, glob", 2);
        assert!(matches!(err, Error::ModuleError(_)), "got {err:?}");
        let message = err.to_string();
        assert!(message.contains("dirsql_path"), "got: {message}");
        assert!(message.contains("at least 4 arguments"), "got: {message}");
        assert!(message.contains("(root, glob)"), "got: {message}");
        assert!(message.contains("got 2"), "got: {message}");
    }

    #[test]
    fn no_scope_error_names_the_module_and_the_loader_to_use() {
        let err = no_scope_error("dirsql_path");
        assert!(matches!(err, Error::ModuleError(_)), "got {err:?}");
        let message = err.to_string();
        assert!(message.contains("dirsql_path"), "got: {message}");
        assert!(
            message.contains("dirsql::vtab::load_module"),
            "got: {message}"
        );
    }

    #[test]
    fn compile_glob_accepts_a_valid_pattern() {
        assert!(compile_glob("**/*.md").unwrap().is_match(Path::new("a.md")));
    }

    #[test]
    fn compile_glob_rejects_an_invalid_pattern() {
        let err = compile_glob("[").unwrap_err();
        assert!(
            matches!(err, Error::ModuleError(_)),
            "invalid globs surface as module errors, got {err:?}"
        );
    }

    #[test]
    fn compile_ignore_accepts_an_empty_pattern_list() {
        assert!(!compile_ignore(&[]).unwrap().is_ignored(Path::new("a.md")));
    }

    #[test]
    fn compile_ignore_applies_each_pattern() {
        let matcher = compile_ignore(&["node_modules/**".to_string()]).unwrap();
        assert!(matcher.is_ignored(Path::new("node_modules/a.js")));
        assert!(!matcher.is_ignored(Path::new("docs/a.md")));
    }

    #[test]
    fn compile_ignore_rejects_an_invalid_pattern() {
        let err = compile_ignore(&["[".to_string()])
            .err()
            .expect("an invalid pattern must be rejected");
        assert!(matches!(err, Error::ModuleError(_)), "got {err:?}");
    }

    #[test]
    fn parse_gitignore_reads_both_switches() {
        assert!(parse_gitignore("gitignore").unwrap());
        assert!(!parse_gitignore("no-gitignore").unwrap());
    }

    #[test]
    fn parse_gitignore_rejects_an_unknown_switch() {
        let err = parse_gitignore("sometimes").expect_err("an unknown switch must be rejected");
        assert!(matches!(err, Error::ModuleError(_)), "got {err:?}");
        assert!(err.to_string().contains("no-gitignore"), "got: {err}");
    }

    #[test]
    fn scan_cost_is_the_declared_flat_cost() {
        assert_eq!(SCAN_COST, 1000.);
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
    fn at_eof_is_true_for_an_empty_scan() {
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

    #[test]
    fn a_row_cache_scans_once_and_serves_the_same_rows_after() {
        let cache: RowCache<u8> = RowCache::default();
        let mut scans = 0;
        let first = cache.rows_or_else(|| {
            scans += 1;
            Arc::new(vec![1, 2])
        });
        let second = cache.rows_or_else(|| {
            scans += 1;
            Arc::new(Vec::new())
        });
        assert_eq!(scans, 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn an_evicted_row_cache_scans_again() {
        let cache: RowCache<u8> = RowCache::default();
        cache.rows_or_else(|| Arc::new(vec![1]));
        cache.evict();
        let rows = cache.rows_or_else(|| Arc::new(vec![2, 3]));
        assert_eq!(*rows, vec![2, 3]);
    }

    fn adopted(scope: &StatementScope) -> Arc<RowCache<u8>> {
        let cache = Arc::new(RowCache::default());
        scope.adopt(Arc::downgrade(&cache) as Weak<dyn Evict + Send + Sync>);
        cache
    }

    #[test]
    fn a_scope_reset_evicts_every_cache_it_adopted() {
        let scope = StatementScope::new();
        let a = adopted(&scope);
        let b = adopted(&scope);
        a.rows_or_else(|| Arc::new(vec![1]));
        b.rows_or_else(|| Arc::new(vec![2]));

        scope.reset();

        assert_eq!(*a.rows_or_else(|| Arc::new(vec![9])), vec![9]);
        assert_eq!(*b.rows_or_else(|| Arc::new(vec![9])), vec![9]);
    }

    #[test]
    fn a_scope_forgets_a_cache_whose_table_is_gone() {
        let scope = StatementScope::new();
        let gone = adopted(&scope);
        let kept = adopted(&scope);
        drop(gone);

        scope.reset();

        assert_eq!(scope.caches.lock().unwrap().len(), 1);
        assert!(Arc::weak_count(&kept) == 1);
    }

    #[test]
    fn entering_a_scope_resets_on_entry_and_on_exit() {
        let scope = StatementScope::new();
        let cache = adopted(&scope);
        cache.rows_or_else(|| Arc::new(vec![1]));

        let guard = scope.enter();
        assert_eq!(*cache.rows_or_else(|| Arc::new(vec![2])), vec![2]);
        drop(guard);
        assert_eq!(*cache.rows_or_else(|| Arc::new(vec![3])), vec![3]);
    }

    struct FakeSource;

    impl TableSource for FakeSource {
        type Row = ();
        const NAME: &'static str = "fake";

        fn connect(_args: &[&[u8]]) -> Result<(String, Self)> {
            unreachable!()
        }

        fn rows(&self) -> Arc<Vec<()>> {
            unreachable!()
        }

        fn column(&self, _row: &(), _ctx: &mut Context, _i: c_int) -> Result<()> {
            unreachable!()
        }
    }

    fn cursor_over(rows: Vec<()>) -> ScaffoldCursor<FakeSource> {
        ScaffoldCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            source: Arc::new(FakeSource),
            cache: Arc::new(RowCache::default()),
            rows: Arc::new(rows),
            index: 0,
        }
    }

    #[test]
    fn cursor_steps_each_row_then_reaches_eof() {
        let mut cursor = cursor_over(vec![(), ()]);

        assert!(!cursor.eof());
        assert_eq!(cursor.rowid().unwrap(), 0);
        cursor.next().unwrap();
        assert!(!cursor.eof());
        assert_eq!(cursor.rowid().unwrap(), 1);
        cursor.next().unwrap();
        assert!(cursor.eof());
        assert_eq!(cursor.rowid().unwrap(), 2);
    }

    #[test]
    fn cursor_is_at_eof_over_no_rows() {
        assert!(cursor_over(Vec::new()).eof());
    }
}
