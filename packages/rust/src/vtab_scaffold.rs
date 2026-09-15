//! The scaffolding both `dirsql_*` virtual tables are built from: decoding the
//! arguments SQLite hands `CREATE VIRTUAL TABLE`, and the read-only table and
//! cursor those arguments drive.
//!
//! A table implements [`TableSource`] and keeps only what actually differs
//! between the two — which module arguments it takes, how it produces a row
//! set, and how a row becomes a cell. Everything either one would otherwise
//! write twice lives here.

use std::ffi::c_int;
use std::sync::Arc;

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
    type Row;

    /// SQL module name the table is created with.
    const NAME: &'static str;

    /// Decode the module arguments into the table's source and the schema it
    /// declares to SQLite.
    fn connect(args: &[&[u8]]) -> Result<(String, Self)>;

    /// The row set one statement reads, taken fresh on every `xFilter`. A
    /// table whose reads are live rescans here; one that materialized at
    /// `CREATE` hands back the `Arc` it already holds.
    fn rows(&self) -> Arc<Vec<Self::Row>>;

    /// Emit column `i` of `row`.
    fn column(&self, row: &Self::Row, ctx: &mut Context, i: c_int) -> Result<()>;
}

/// Register `S` as a virtual-table module on `conn`.
pub fn load_module<S: TableSource>(conn: &Connection) -> Result<()> {
    let aux: Option<()> = None;
    conn.create_module(S::NAME, read_only_module::<ScaffoldTab<S>>(), aux)
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
}

#[expect(unsafe_code, reason = "rusqlite requires an unsafe trait impl")]
unsafe impl<'vtab, S: TableSource> VTab<'vtab> for ScaffoldTab<S> {
    type Aux = ();
    type Cursor = ScaffoldCursor<S>;

    fn connect(
        _db: &mut VTabConnection,
        _aux: Option<&()>,
        args: &[&[u8]],
    ) -> Result<(String, Self)> {
        let (schema, source) = S::connect(args)?;
        let vtab = Self {
            base: ffi::sqlite3_vtab::default(),
            source: Arc::new(source),
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
        self.rows = self.source.rows();
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
        let err = parse_gitignore("sometimes")
            .err()
            .expect("an unknown switch must be rejected");
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
