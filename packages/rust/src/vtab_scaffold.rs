//! The scaffolding both `dirsql_*` virtual tables are built from: decoding the
//! arguments SQLite hands `CREATE VIRTUAL TABLE`, and the read-only table and
//! cursor those arguments drive.
//!
//! A table implements [`TableSource`] and keeps only what actually differs
//! between the two — which module arguments it takes, how it produces a row
//! set, which columns answer an equality by lookup, and how a row becomes a
//! cell. Everything either one would otherwise write twice lives here.

use std::collections::{HashMap, HashSet};
use std::ffi::c_int;
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};

use rusqlite::types::ValueRef;
use rusqlite::vtab::{
    Context, CreateVTab, IndexConstraintOp, IndexInfo, VTab, VTabConnection, VTabCursor, VTabKind,
    Values, read_only_module,
};
use rusqlite::{Connection, Error, Result, ffi};

use crate::Value;
use crate::matcher::TableMatcher;
use crate::scanner::{self, PathGlob};
use crate::sql_literal::unquote;

/// Cost a full scan is declared to carry: enough below SQLite's near-infinite
/// default that the planner does not reorder a join around the table.
const SCAN_COST: f64 = 1000.;

/// Cost of answering one equality by lookup. Far enough under [`SCAN_COST`]
/// that SQLite puts a table it can probe on the inner side of a join.
const LOOKUP_COST: f64 = 10.;

/// `idxNum` for a full scan; a lookup carries its column as `column + 1`.
const SCAN_IDX: c_int = 0;

/// What a `dirsql_*` table supplies on top of the scaffolding.
pub trait TableSource: Sized + Send + Sync + 'static {
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

    /// Columns an equality constraint is answered on by lookup rather than by
    /// scanning, in order of preference.
    const LOOKUP_COLUMNS: &'static [usize] = &[];

    /// The text `row` carries in lookup column `column`; `None` for a cell
    /// that is not text, which no equality can match. Only reached through a
    /// column in [`Self::LOOKUP_COLUMNS`], so a source that declares none is
    /// never asked.
    fn lookup_key<'r>(&self, row: &'r Self::Row, column: usize) -> Option<&'r str> {
        let _ = row;
        unreachable!("column {column} is not a lookup column of {}", Self::NAME)
    }

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
///
/// SQLite also opens those cursors one at a time, so left to itself a
/// statement over two tables walks the second tree only after the first is
/// done. [`warm`](Self::warm) starts the scans of the tables a statement
/// names before SQLite asks for any of them, each on a thread of its own.
#[derive(Default)]
pub struct StatementScope {
    tables: Mutex<Vec<(String, Weak<dyn Held + Send + Sync>)>>,
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
        let mut tables = self.tables.lock().unwrap();
        tables.retain(|(_, cache)| {
            cache.upgrade().is_some_and(|cache| {
                cache.evict();
                true
            })
        });
    }

    /// Start scanning every table named in `names` now, so the statement
    /// about to read them finds their rows underway. A name no table here
    /// answers to is passed over; a table already holding rows keeps them.
    pub fn warm(&self, names: &HashSet<String>) {
        for (name, cache) in self.tables.lock().unwrap().iter() {
            if names.contains(name)
                && let Some(cache) = cache.upgrade()
            {
                cache.warm();
            }
        }
    }

    fn adopt(&self, name: String, cache: Weak<dyn Held + Send + Sync>) {
        self.tables.lock().unwrap().push((name, cache));
    }
}

pub struct StatementGuard(Arc<StatementScope>);

impl Drop for StatementGuard {
    fn drop(&mut self) {
        self.0.reset();
    }
}

trait Held {
    fn evict(&self);
    fn warm(&self);
}

/// Where one table's rows stand within the statement in progress.
enum Slot<R> {
    Empty,
    /// A scan started ahead of the first read, on a thread of its own.
    Pending(JoinHandle<Arc<Vec<R>>>),
    Ready(Arc<Vec<R>>),
}

/// One table's rows for the statement in progress, empty between statements.
struct RowCache<S: TableSource> {
    source: Arc<S>,
    slot: Mutex<Slot<S::Row>>,
}

impl<S: TableSource> RowCache<S> {
    fn new(source: Arc<S>) -> Self {
        Self {
            source,
            slot: Mutex::new(Slot::Empty),
        }
    }

    /// The rows held, waiting for a scan started ahead to finish; or else the
    /// source's rows scanned now, held from here on.
    fn rows(&self) -> Arc<Vec<S::Row>> {
        let mut slot = self.slot.lock().unwrap();
        let rows = match std::mem::replace(&mut *slot, Slot::Empty) {
            Slot::Ready(rows) => rows,
            Slot::Pending(scan) => scan
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            Slot::Empty => self.source.rows(),
        };
        *slot = Slot::Ready(Arc::clone(&rows));
        rows
    }

    /// Start the scan on a thread of its own, unless rows are already held
    /// or underway.
    fn warm(&self) {
        let mut slot = self.slot.lock().unwrap();
        if !matches!(*slot, Slot::Empty) {
            return;
        }
        let source = Arc::clone(&self.source);
        if let Ok(scan) = thread::Builder::new().spawn(move || source.rows()) {
            *slot = Slot::Pending(scan);
        }
    }
}

impl<S: TableSource> Held for RowCache<S> {
    fn evict(&self) {
        *self.slot.lock().unwrap() = Slot::Empty;
    }

    fn warm(&self) {
        self.warm();
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

/// The table's name, the third argument SQLite prepends: the same spelling
/// its authorizer reports a read of the table under.
fn table_name(args: &[&[u8]]) -> String {
    args.get(2)
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .unwrap_or_default()
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
pub fn compile_glob(pattern: &str) -> Result<PathGlob> {
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

/// One `WHERE` term SQLite offers `xBestIndex`, reduced to what the choice
/// of lookup depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Constraint {
    column: c_int,
    usable: bool,
    equality: bool,
}

/// The constraint a lookup answers and the column it probes.
#[derive(Debug, PartialEq)]
struct Lookup {
    constraint: usize,
    column: usize,
}

/// The usable equality on the most preferred lookup column, if any.
fn choose_lookup(lookup_columns: &[usize], constraints: &[Constraint]) -> Option<Lookup> {
    lookup_columns.iter().find_map(|&column| {
        let wanted = c_int::try_from(column).ok()?;
        let constraint = constraints
            .iter()
            .position(|c| c.usable && c.equality && c.column == wanted)?;
        Some(Lookup { constraint, column })
    })
}

/// The `idxNum` that names a lookup on `column`.
fn lookup_idx_num(column: usize) -> c_int {
    column
        .checked_add(1)
        .and_then(|n| c_int::try_from(n).ok())
        .unwrap_or(SCAN_IDX)
}

/// The lookup column an `idxNum` names; `None` for a full scan.
fn lookup_column(idx_num: c_int) -> Option<usize> {
    usize::try_from(idx_num).ok()?.checked_sub(1)
}

/// The text of a filter argument; `None` when it is not text, in which case
/// the cursor falls back to every row and SQLite applies the constraint.
fn text_key(value: ValueRef<'_>) -> Option<&str> {
    value.as_str().ok()
}

/// Row positions keyed by one column's text, built once per cursor from its
/// scanned rows.
struct LookupIndex {
    column: usize,
    positions: HashMap<String, Vec<usize>>,
}

impl LookupIndex {
    fn build<'r, R: 'r>(
        rows: impl Iterator<Item = &'r R>,
        column: usize,
        key: impl Fn(&'r R, usize) -> Option<&'r str>,
    ) -> Self {
        let mut positions: HashMap<String, Vec<usize>> = HashMap::new();
        for (position, row) in rows.enumerate() {
            if let Some(text) = key(row, column) {
                positions.entry(text.to_owned()).or_default().push(position);
            }
        }
        Self { column, positions }
    }

    fn matching(&self, key: &str) -> Vec<usize> {
        self.positions.get(key).cloned().unwrap_or_default()
    }
}

/// The rows one `xFilter` selected, as positions into the cursor's row set.
enum Selection {
    Every(usize),
    Only(Vec<usize>),
}

impl Selection {
    fn len(&self) -> usize {
        match self {
            Self::Every(count) => *count,
            Self::Only(positions) => positions.len(),
        }
    }

    /// The row position the `step`th selected row sits at.
    fn position(&self, step: usize) -> Option<usize> {
        match self {
            Self::Every(count) => (step < *count).then_some(step),
            Self::Only(positions) => positions.get(step).copied(),
        }
    }
}

/// Whether the cursor has run past the last selected row.
fn at_eof(step: usize, selected: usize) -> bool {
    step >= selected
}

/// Rowid for a row position, saturating rather than wrapping on a row count
/// no filesystem will produce.
fn rowid_of(position: usize) -> i64 {
    i64::try_from(position).unwrap_or(i64::MAX)
}

#[repr(C)]
pub struct ScaffoldTab<S: TableSource> {
    /// Base class. Must be first.
    base: ffi::sqlite3_vtab,
    source: Arc<S>,
    cache: Arc<RowCache<S>>,
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
        let source = Arc::new(source);
        let cache = Arc::new(RowCache::new(Arc::clone(&source)));
        scope.adopt(
            table_name(args),
            Arc::downgrade(&cache) as Weak<dyn Held + Send + Sync>,
        );
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
            source,
            cache,
        };
        Ok((schema, vtab))
    }

    fn best_index(&self, info: &mut IndexInfo) -> Result<()> {
        let constraints: Vec<Constraint> = info
            .constraints()
            .map(|c| Constraint {
                column: c.column(),
                usable: c.is_usable(),
                equality: c.operator() == IndexConstraintOp::SQLITE_INDEX_CONSTRAINT_EQ,
            })
            .collect();

        match choose_lookup(S::LOOKUP_COLUMNS, &constraints) {
            Some(lookup) => {
                info.constraint_usage(lookup.constraint).set_argv_index(1);
                info.set_idx_num(lookup_idx_num(lookup.column));
                info.set_estimated_cost(LOOKUP_COST);
            }
            None => {
                info.set_idx_num(SCAN_IDX);
                info.set_estimated_cost(SCAN_COST);
            }
        }
        Ok(())
    }

    fn open(&'vtab mut self) -> Result<ScaffoldCursor<S>> {
        Ok(ScaffoldCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            source: Arc::clone(&self.source),
            cache: Arc::clone(&self.cache),
            rows: None,
            index: None,
            selection: Selection::Every(0),
            step: 0,
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
    cache: Arc<RowCache<S>>,
    /// Taken from the statement's cache on the first `xFilter`; `None` until
    /// then.
    rows: Option<Arc<Vec<S::Row>>>,
    /// Built on the first `xFilter` that probes a column.
    index: Option<LookupIndex>,
    selection: Selection,
    step: usize,
}

impl<S: TableSource> ScaffoldCursor<S> {
    fn rows(&mut self) -> Arc<Vec<S::Row>> {
        let cache = &self.cache;
        Arc::clone(self.rows.get_or_insert_with(|| cache.rows()))
    }

    fn select_by_lookup(&mut self, column: usize, key: &str) -> Selection {
        let rows = self.rows();
        let source = Arc::clone(&self.source);
        let index = match &self.index {
            Some(index) if index.column == column => index,
            _ => self
                .index
                .insert(LookupIndex::build(rows.iter(), column, |row, c| {
                    source.lookup_key(row, c)
                })),
        };
        Selection::Only(index.matching(key))
    }

    fn current(&self) -> Option<(usize, &S::Row)> {
        let position = self.selection.position(self.step)?;
        let row = self.rows.as_ref()?.get(position)?;
        Some((position, row))
    }
}

#[expect(unsafe_code, reason = "rusqlite requires an unsafe trait impl")]
unsafe impl<S: TableSource> VTabCursor for ScaffoldCursor<S> {
    fn filter(&mut self, idx_num: c_int, _idx_str: Option<&str>, args: &Values<'_>) -> Result<()> {
        let key = args.iter().next().and_then(text_key);
        self.selection = match (lookup_column(idx_num), key) {
            (Some(column), Some(key)) => self.select_by_lookup(column, key),
            _ => Selection::Every(self.rows().len()),
        };
        self.step = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<()> {
        self.step += 1;
        Ok(())
    }

    fn eof(&self) -> bool {
        at_eof(self.step, self.selection.len())
    }

    fn column(&self, ctx: &mut Context, i: c_int) -> Result<()> {
        match self.current() {
            Some((_, row)) => self.source.column(row, ctx, i),
            None => ctx.set_result(&Value::Null),
        }
    }

    fn rowid(&self) -> Result<i64> {
        Ok(rowid_of(
            self.current().map_or(self.step, |(position, _)| position),
        ))
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
    fn a_lookup_is_declared_far_cheaper_than_a_scan() {
        assert_eq!(LOOKUP_COST, 10.);
    }

    #[test]
    fn a_source_declares_no_lookup_columns_by_default() {
        assert!(FakeSource::LOOKUP_COLUMNS.is_empty());
    }

    fn equality_on(column: c_int) -> Constraint {
        Constraint {
            column,
            usable: true,
            equality: true,
        }
    }

    #[test]
    fn choose_lookup_takes_a_usable_equality_on_a_lookup_column() {
        let chosen = choose_lookup(&[2], &[equality_on(5), equality_on(2)]);
        assert_eq!(
            chosen,
            Some(Lookup {
                constraint: 1,
                column: 2
            })
        );
    }

    #[test]
    fn choose_lookup_prefers_the_earlier_lookup_column() {
        let chosen = choose_lookup(&[0, 2], &[equality_on(2), equality_on(0)]);
        assert_eq!(
            chosen,
            Some(Lookup {
                constraint: 1,
                column: 0
            })
        );
    }

    #[test]
    fn choose_lookup_falls_through_to_a_later_lookup_column() {
        let chosen = choose_lookup(&[0, 2], &[equality_on(2)]);
        assert_eq!(
            chosen,
            Some(Lookup {
                constraint: 0,
                column: 2
            })
        );
    }

    #[test]
    fn choose_lookup_skips_an_unusable_constraint() {
        let unusable = Constraint {
            usable: false,
            ..equality_on(2)
        };
        assert_eq!(choose_lookup(&[2], &[unusable]), None);
    }

    #[test]
    fn choose_lookup_skips_a_non_equality_constraint() {
        let range = Constraint {
            equality: false,
            ..equality_on(2)
        };
        assert_eq!(choose_lookup(&[2], &[range]), None);
    }

    #[test]
    fn choose_lookup_ignores_a_column_the_source_does_not_index() {
        assert_eq!(choose_lookup(&[2], &[equality_on(3)]), None);
        assert_eq!(choose_lookup(&[], &[equality_on(2)]), None);
    }

    #[test]
    fn choose_lookup_never_matches_the_rowid_pseudo_column() {
        assert_eq!(choose_lookup(&[usize::MAX], &[equality_on(-1)]), None);
    }

    #[test]
    fn lookup_idx_num_round_trips_through_lookup_column() {
        for column in [0, 1, 2, 7] {
            assert_eq!(lookup_column(lookup_idx_num(column)), Some(column));
        }
    }

    #[test]
    fn lookup_idx_num_is_never_the_scan_index() {
        assert_ne!(lookup_idx_num(0), SCAN_IDX);
        assert_eq!(lookup_idx_num(0), 1);
        assert_eq!(lookup_idx_num(2), 3);
    }

    #[test]
    fn lookup_idx_num_saturates_to_a_scan_on_overflow() {
        assert_eq!(lookup_idx_num(usize::MAX), SCAN_IDX);
    }

    #[test]
    fn the_scan_index_names_no_lookup_column() {
        assert_eq!(SCAN_IDX, 0);
        assert_eq!(lookup_column(SCAN_IDX), None);
        assert_eq!(lookup_column(-1), None);
    }

    #[test]
    fn text_key_reads_a_text_argument() {
        assert_eq!(text_key(ValueRef::Text(b"docs")), Some("docs"));
    }

    #[test]
    fn text_key_is_none_for_a_non_text_argument() {
        assert_eq!(text_key(ValueRef::Integer(5)), None);
        assert_eq!(text_key(ValueRef::Null), None);
        assert_eq!(text_key(ValueRef::Blob(b"docs")), None);
    }

    fn by_first_letter<'r>(row: &'r &str, column: usize) -> Option<&'r str> {
        (column == 1).then(|| &row[..1])
    }

    #[test]
    fn lookup_index_groups_row_positions_by_key() {
        let rows = ["ab", "cd", "ax"];
        let index = LookupIndex::build(rows.iter(), 1, by_first_letter);

        assert_eq!(index.column, 1);
        assert_eq!(index.matching("a"), vec![0, 2]);
        assert_eq!(index.matching("c"), vec![1]);
    }

    #[test]
    fn lookup_index_matches_nothing_for_an_absent_key() {
        let rows = ["ab"];
        let index = LookupIndex::build(rows.iter(), 1, by_first_letter);
        assert_eq!(index.matching("z"), Vec::<usize>::new());
    }

    #[test]
    fn lookup_index_skips_rows_without_a_key() {
        let rows = ["ab", "cd"];
        let index = LookupIndex::build(rows.iter(), 0, by_first_letter);
        assert!(index.positions.is_empty());
    }

    #[test]
    fn selecting_every_row_walks_the_positions_in_order() {
        let every = Selection::Every(2);
        assert_eq!(every.len(), 2);
        assert_eq!(every.position(0), Some(0));
        assert_eq!(every.position(1), Some(1));
        assert_eq!(every.position(2), None);
    }

    #[test]
    fn selecting_only_some_rows_maps_steps_to_their_positions() {
        let only = Selection::Only(vec![4, 9]);
        assert_eq!(only.len(), 2);
        assert_eq!(only.position(0), Some(4));
        assert_eq!(only.position(1), Some(9));
        assert_eq!(only.position(2), None);
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

    /// A source whose rows are the number of scans it has served, so a test
    /// can tell a fresh scan from rows held over.
    struct Counted {
        scans: std::sync::atomic::AtomicUsize,
    }

    impl TableSource for Counted {
        type Row = usize;
        const NAME: &'static str = "counted";

        fn connect(_args: &[&[u8]]) -> Result<(String, Self)> {
            unreachable!()
        }

        fn rows(&self) -> Arc<Vec<usize>> {
            let scan = self
                .scans
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            Arc::new(vec![scan])
        }

        fn column(&self, _row: &usize, _ctx: &mut Context, _i: c_int) -> Result<()> {
            unreachable!()
        }
    }

    fn counted() -> RowCache<Counted> {
        RowCache::new(Arc::new(Counted {
            scans: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    fn scans(cache: &RowCache<Counted>) -> usize {
        cache
            .source
            .scans
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn is_pending(cache: &RowCache<Counted>) -> bool {
        matches!(*cache.slot.lock().unwrap(), Slot::Pending(_))
    }

    #[test]
    fn a_row_cache_scans_once_and_serves_the_same_rows_after() {
        let cache = counted();
        let first = cache.rows();
        let second = cache.rows();
        assert_eq!(scans(&cache), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn an_evicted_row_cache_scans_again() {
        let cache = counted();
        cache.rows();
        cache.evict();
        assert_eq!(*cache.rows(), vec![2]);
    }

    #[test]
    fn a_warmed_row_cache_has_its_scan_underway_before_any_read() {
        let cache = counted();
        cache.warm();
        assert!(is_pending(&cache));
        assert_eq!(*cache.rows(), vec![1]);
        assert_eq!(scans(&cache), 1);
    }

    #[test]
    fn warming_twice_starts_one_scan() {
        let cache = counted();
        cache.warm();
        cache.warm();
        assert_eq!(*cache.rows(), vec![1]);
        assert_eq!(scans(&cache), 1);
    }

    #[test]
    fn warming_a_row_cache_holding_rows_keeps_them() {
        let cache = counted();
        let held = cache.rows();
        cache.warm();
        assert!(!is_pending(&cache));
        assert!(Arc::ptr_eq(&held, &cache.rows()));
        assert_eq!(scans(&cache), 1);
    }

    #[test]
    fn a_read_after_a_warmed_scan_serves_that_scan_again() {
        let cache = counted();
        cache.warm();
        let first = cache.rows();
        assert!(Arc::ptr_eq(&first, &cache.rows()));
    }

    fn adopted(scope: &StatementScope, name: &str) -> Arc<RowCache<Counted>> {
        let cache = Arc::new(counted());
        scope.adopt(
            name.to_string(),
            Arc::downgrade(&cache) as Weak<dyn Held + Send + Sync>,
        );
        cache
    }

    fn names(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_scope_reset_evicts_every_cache_it_adopted() {
        let scope = StatementScope::new();
        let a = adopted(&scope, "a");
        let b = adopted(&scope, "b");
        a.rows();
        b.rows();

        scope.reset();

        assert_eq!(*a.rows(), vec![2]);
        assert_eq!(*b.rows(), vec![2]);
    }

    #[test]
    fn a_scope_forgets_a_cache_whose_table_is_gone() {
        let scope = StatementScope::new();
        let gone = adopted(&scope, "gone");
        let kept = adopted(&scope, "kept");
        drop(gone);

        scope.reset();

        assert_eq!(scope.tables.lock().unwrap().len(), 1);
        assert!(Arc::weak_count(&kept) == 1);
    }

    #[test]
    fn a_scope_warms_the_tables_named_and_no_other() {
        let scope = StatementScope::new();
        let named = adopted(&scope, "named");
        let other = adopted(&scope, "other");

        scope.warm(&names(&["named", "absent"]));

        assert!(is_pending(&named));
        assert!(!is_pending(&other));
        assert_eq!(scans(&other), 0);
    }

    #[test]
    fn a_scope_warms_a_table_by_its_own_name_only() {
        let scope = StatementScope::new();
        let table = adopted(&scope, "./a");

        scope.warm(&names(&["./b"]));

        assert!(!is_pending(&table));
    }

    #[test]
    fn entering_a_scope_resets_on_entry_and_on_exit() {
        let scope = StatementScope::new();
        let cache = adopted(&scope, "t");
        cache.rows();

        let guard = scope.enter();
        assert_eq!(*cache.rows(), vec![2]);
        drop(guard);
        assert_eq!(*cache.rows(), vec![3]);
    }

    #[test]
    fn table_name_is_the_third_argument_sqlite_prepends() {
        assert_eq!(
            table_name(&[b"dirsql_path", b"temp", b"./a", b"'/root'"]),
            "./a"
        );
    }

    #[test]
    fn table_name_is_empty_when_sqlite_prepends_too_few() {
        assert_eq!(table_name(&[b"dirsql_path"]), "");
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

    /// A source whose rows are the words it was given, looked up on column 1
    /// by first letter. Counts its scans so a test can see how many a cursor
    /// asked for.
    struct Words {
        words: Vec<&'static str>,
        scans: std::sync::atomic::AtomicUsize,
        keys: std::sync::atomic::AtomicUsize,
    }

    thread_local! {
        /// Reads of the `initial` column through SQLite on this thread: one
        /// per row the cursor hands back, since SQLite re-checks the equality.
        static INITIAL_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    impl TableSource for Words {
        type Row = &'static str;
        const NAME: &'static str = "words";
        const LOOKUP_COLUMNS: &'static [usize] = &[1];

        fn connect(_args: &[&[u8]]) -> Result<(String, Self)> {
            let words = Words {
                words: vec!["ant", "bat", "bee"],
                scans: std::sync::atomic::AtomicUsize::new(0),
                keys: std::sync::atomic::AtomicUsize::new(0),
            };
            Ok(("CREATE TABLE x(word TEXT, initial TEXT)".to_string(), words))
        }

        fn rows(&self) -> Arc<Vec<&'static str>> {
            self.scans
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Arc::new(self.words.clone())
        }

        fn column(&self, row: &&'static str, ctx: &mut Context, i: c_int) -> Result<()> {
            if i == 0 {
                return ctx.set_result(&Value::Text(row.to_string()));
            }
            INITIAL_READS.with(|reads| reads.set(reads.get() + 1));
            ctx.set_result(&Value::Text(row[..1].to_string()))
        }

        fn lookup_key<'r>(&self, row: &'r &'static str, column: usize) -> Option<&'r str> {
            self.keys.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (column == 1).then(|| &row[..1])
        }
    }

    fn cursor_over(words: Vec<&'static str>) -> ScaffoldCursor<Words> {
        let source = Arc::new(Words {
            words,
            scans: std::sync::atomic::AtomicUsize::new(0),
            keys: std::sync::atomic::AtomicUsize::new(0),
        });
        ScaffoldCursor {
            base: ffi::sqlite3_vtab_cursor::default(),
            cache: Arc::new(RowCache::new(Arc::clone(&source))),
            source,
            rows: None,
            index: None,
            selection: Selection::Every(0),
            step: 0,
        }
    }

    fn selected(cursor: &ScaffoldCursor<Words>) -> Vec<&'static str> {
        (0..cursor.selection.len())
            .map(|step| cursor.selection.position(step).unwrap())
            .map(|position| cursor.rows.as_ref().unwrap()[position])
            .collect()
    }

    #[test]
    fn an_unfiltered_cursor_is_at_eof_before_its_first_filter() {
        let cursor = cursor_over(vec!["ab"]);
        assert!(cursor.eof());
        assert!(cursor.current().is_none());
        assert_eq!(cursor.rowid().unwrap(), 0);
    }

    #[test]
    fn a_scan_selects_every_row_in_order() {
        let mut cursor = cursor_over(vec!["ab", "cd"]);
        cursor.selection = Selection::Every(cursor.rows().len());
        cursor.step = 0;

        assert_eq!(selected(&cursor), vec!["ab", "cd"]);
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
    fn a_scan_over_no_rows_is_at_eof() {
        let mut cursor = cursor_over(Vec::new());
        cursor.selection = Selection::Every(cursor.rows().len());
        assert!(cursor.eof());
    }

    #[test]
    fn a_lookup_selects_only_the_rows_carrying_the_key() {
        let mut cursor = cursor_over(vec!["ab", "cd", "ax"]);
        cursor.selection = cursor.select_by_lookup(1, "a");

        assert_eq!(selected(&cursor), vec!["ab", "ax"]);
    }

    #[test]
    fn a_lookup_keeps_the_scan_position_as_the_rowid() {
        let mut cursor = cursor_over(vec!["ab", "cd", "ax"]);
        cursor.selection = cursor.select_by_lookup(1, "a");
        cursor.next().unwrap();

        assert_eq!(cursor.current().map(|(_, row)| *row), Some("ax"));
        assert_eq!(cursor.rowid().unwrap(), 2);
    }

    #[test]
    fn a_lookup_for_an_absent_key_is_at_eof() {
        let mut cursor = cursor_over(vec!["ab"]);
        cursor.selection = cursor.select_by_lookup(1, "z");
        assert!(cursor.eof());
    }

    #[test]
    fn a_cursor_scans_its_source_once() {
        let mut cursor = cursor_over(vec!["ab", "cd"]);
        cursor.rows();
        cursor.select_by_lookup(1, "a");
        cursor.select_by_lookup(1, "c");

        assert_eq!(
            cursor
                .source
                .scans
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn a_cursor_builds_its_index_once_per_column() {
        let mut cursor = cursor_over(vec!["ab", "cd"]);
        cursor.select_by_lookup(1, "a");
        let first = cursor.index.as_ref().unwrap() as *const LookupIndex;
        cursor.select_by_lookup(1, "c");
        let second = cursor.index.as_ref().unwrap() as *const LookupIndex;

        assert_eq!(first, second);
        assert_eq!(cursor.index.as_ref().unwrap().column, 1);
    }

    #[test]
    fn a_cursor_rebuilds_its_index_for_a_different_column() {
        let mut cursor = cursor_over(vec!["ab", "cd"]);
        cursor.select_by_lookup(1, "a");
        cursor.selection = cursor.select_by_lookup(0, "a");

        assert_eq!(cursor.index.as_ref().unwrap().column, 0);
        assert!(cursor.eof(), "column 0 carries no key, so nothing matches");
    }

    #[test]
    #[should_panic(expected = "not a lookup column")]
    fn a_source_without_lookup_columns_is_never_asked_for_a_key() {
        FakeSource.lookup_key(&(), 0);
    }

    fn words_table() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        load_module::<Words>(&conn, StatementScope::new()).unwrap();
        conn.execute_batch("CREATE VIRTUAL TABLE w USING words")
            .unwrap();
        conn
    }

    fn strings(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    fn plan(conn: &Connection, sql: &str) -> Vec<String> {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    #[test]
    fn sqlite_plans_an_equality_on_a_lookup_column_as_a_lookup() {
        let conn = words_table();

        assert_eq!(
            plan(&conn, "SELECT word FROM w WHERE initial = 'b'"),
            vec!["SCAN w VIRTUAL TABLE INDEX 2:"]
        );
    }

    #[test]
    fn sqlite_plans_an_equality_on_another_column_as_a_scan() {
        let conn = words_table();

        assert_eq!(
            plan(&conn, "SELECT word FROM w WHERE word = 'bat'"),
            vec!["SCAN w VIRTUAL TABLE INDEX 0:"]
        );
    }

    #[test]
    fn sqlite_reads_the_looked_up_rows() {
        let conn = words_table();

        assert_eq!(
            strings(
                &conn,
                "SELECT word FROM w WHERE initial = 'b' ORDER BY word"
            ),
            vec!["bat", "bee"]
        );
    }

    #[test]
    fn sqlite_reads_every_row_of_a_scan() {
        let conn = words_table();

        assert_eq!(
            strings(&conn, "SELECT word || initial FROM w ORDER BY word"),
            vec!["anta", "batb", "beeb"]
        );
    }

    #[test]
    fn a_second_lookup_on_the_same_column_reuses_the_index() {
        let mut cursor = cursor_over(vec!["ant", "bat", "bee"]);

        cursor.select_by_lookup(1, "b");
        cursor.select_by_lookup(1, "a");

        let keyed = cursor
            .source
            .keys
            .load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            keyed, 3,
            "each row is keyed once per column, not per lookup"
        );
    }

    #[test]
    fn sqlite_is_handed_only_the_looked_up_rows() {
        let conn = words_table();
        INITIAL_READS.with(|reads| reads.set(0));

        strings(&conn, "SELECT word FROM w WHERE initial = 'b'");

        assert_eq!(INITIAL_READS.with(std::cell::Cell::get), 2);
    }
}
