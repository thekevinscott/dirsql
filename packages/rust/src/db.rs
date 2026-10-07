use rusqlite::Connection;
use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

use crate::functions::{self, CallReporter, ResolvedFunction, Worker};
use crate::infer::JsonRow;
use crate::parsed_vtab;
use crate::path_table::{self, DEFAULT_IGNORES, PathTable, Resolution};
use crate::scanner;
use crate::sql_literal::{quote_identifier, quote_literal};
use crate::vtab::{self, StatementScope};

/// The user's home directory, if the platform reports one. Injected here so
/// the `~/` rule has a single production source.
pub(crate) fn home_dir() -> Option<PathBuf> {
    #[allow(deprecated)]
    std::env::home_dir()
}

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("invalid identifier: {0:?} (must match [A-Za-z_][A-Za-z0-9_]*)")]
    InvalidIdentifier(String),

    #[error(
        "query() only accepts read-only statements; SQLite classified this statement as a write"
    )]
    WriteForbidden,

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    PathTable(String),
}

/// The reserved namespace for dirsql's internal bookkeeping tables. Every
/// engine table (`_dirsql_internal_rows`, `_dirsql_files`, `_dirsql_meta`, and
/// any future sibling) lives here; [`Db::query`] treats the whole namespace as
/// unreachable, so the internal schema is a genuine private surface.
const INTERNAL_TABLE_PREFIX: &str = "_dirsql_";

/// The message carried by [`DbError::Unauthorized`] when `query()` rejects a
/// read of an internal table.
const INTERNAL_TABLE_DENIED_MSG: &str = "not authorized: dirsql's internal bookkeeping tables (the `_dirsql_*` namespace) \
     are not readable through query()";

/// The message carried by [`DbError::Unauthorized`] when `query()` rejects an
/// `ATTACH`/`DETACH`. SQLite classifies both as read-only, so the `readonly()`
/// gate lets them through; they are effectful (ATTACH creates/opens an
/// arbitrary database file) and denied at prepare time instead.
const ATTACH_DENIED_MSG: &str = "not authorized: query() does not permit ATTACH or DETACH; \
     attaching external databases is disabled on this surface";

fn is_internal_table(name: &str) -> bool {
    name.starts_with(INTERNAL_TABLE_PREFIX)
}

/// The prefix SQLite puts on the one prepare error a path-table can rescue.
const NO_SUCH_TABLE: &str = "no such table: ";

/// Extract the table name from a SQLite prepare error message.
///
/// This is the *only* discovery mechanism: dirsql never parses SQL, so joins,
/// subqueries and CTEs work for free — SQLite names each missing target in turn.
fn missing_table_name(message: &str) -> Option<&str> {
    let name = message.strip_prefix(NO_SUCH_TABLE)?.trim();
    (!name.is_empty()).then_some(name)
}

fn bare_glob_hint(name: &str) -> String {
    format!("{NO_SUCH_TABLE}{name}; did you mean './{name}'?")
}

/// The name the retired implicit no-config table used to have.
const LEGACY_DEFAULT_TABLE: &str = "files";

fn legacy_files_table_hint() -> String {
    format!("{NO_SUCH_TABLE}{LEGACY_DEFAULT_TABLE}; did you mean FROM './**'?")
}

/// The characters a bare filesystem path can be spelled with where a table
/// name goes. Widening SQLite's error offset over these recovers the whole
/// path from whichever character its tokenizer happened to reject.
fn is_path_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '/' | '_' | '-' | '~' | '*' | '?')
}

/// The bare path around `offset` in `sql`, if there is one.
///
/// SQLite reports the offset of the character it choked on, which is not where
/// the path starts: `FROM ./` rejects the leading `.`, while `FROM src/main.rs`
/// rejects the `/` in the middle. Widening in both directions recovers the same
/// token either way. A `/` is required, which keeps an ordinary syntax error
/// over an identifier or a number out of the hint.
fn path_token_at(sql: &str, offset: usize) -> Option<&str> {
    if !sql.is_char_boundary(offset) {
        return None;
    }
    let start = sql[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_path_char(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = offset
        + sql[offset..]
            .find(|c: char| !is_path_char(c))
            .unwrap_or(sql.len() - offset);

    let token = &sql[start..end];
    token.contains('/').then_some(token)
}

fn unquoted_path_hint(token: &str) -> String {
    format!("hint: paths used as table names must be quoted; did you mean {token:?}?")
}

pub(crate) fn no_home_path_table(name: &str) -> String {
    format!(
        "path-table {name:?} cannot be resolved: no home directory for '~' \
         (set HOME, or write the path out in full)"
    )
}

/// Wrap `s` as a SQL string literal.
/// The DDL that mints a path-table.
///
/// It lands in `temp`, which is per-connection and never written to the
/// persistent cache file — so a path-table can never leak into a persisted
/// `sqlite_master`. `IF NOT EXISTS` makes a repeat reference a no-op, which is
/// why no cross-call registry is needed.
///
/// With a `parser` present (the CLI's `--on-file`), the table is minted over
/// the parsed module instead: its rows and schema come from the parser, not
/// the stat columns, and the `path_prefix` is irrelevant (a parser wanting a
/// path emits it). Both forms carry the same ignore rules.
///
/// A parsed table also carries `index_root`, where the parser is spawned. The
/// stat module takes no such argument: it spawns nothing.
fn path_table_ddl(
    name: &str,
    table: &PathTable,
    ignore: &[String],
    gitignore: bool,
    parser: Option<&str>,
    index_root: &Path,
) -> String {
    let (module, mut args) = match parser {
        Some(command) => (
            parsed_vtab::MODULE_NAME,
            vec![
                quote_literal(&table.root.to_string_lossy()),
                quote_literal(&table.glob),
                quote_literal(command),
            ],
        ),
        None => (
            vtab::MODULE_NAME,
            vec![
                quote_literal(&table.root.to_string_lossy()),
                quote_literal(&table.glob),
                quote_literal(&table.path_prefix),
            ],
        ),
    };
    args.push(quote_literal(if !gitignore {
        scanner::NO_GITIGNORE_ARG
    } else if !table.path_prefix.is_empty() && path_table::wildcard_below_prefix(name) {
        scanner::GITIGNORE_ROOT_ARG
    } else {
        scanner::GITIGNORE_ARG
    }));
    if parser.is_some() {
        args.push(quote_literal(&index_root.to_string_lossy()));
    }
    args.extend(ignore.iter().map(|p| quote_literal(p)));

    format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.{} USING {}({})",
        quote_identifier(name),
        module,
        args.join(", "),
    )
}

/// Validate that `s` is a safe unquoted SQL identifier: starts with an
/// ASCII letter or underscore, followed by ASCII letters / digits /
/// underscores. Must be called at every entry point that interpolates an
/// identifier into formatted SQL (`INSERT INTO {table} ...`,
/// `PRAGMA table_info({table})`, `INSERT INTO {table} ({col}, ...)`).
pub fn validate_identifier(s: &str) -> Result<()> {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return Err(DbError::InvalidIdentifier(s.to_string())),
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return Err(DbError::InvalidIdentifier(s.to_string()));
        }
    }
    Ok(())
}

pub type Result<T> = std::result::Result<T, DbError>;

const MAPPING_INSERT: &str = "INSERT INTO _dirsql_internal_rows \
     (table_name, file_path, row_index, rowid_ref) VALUES (?1, ?2, ?3, ?4)";

const RANGE_INSERT: &str = "INSERT INTO _dirsql_internal_ranges \
     (table_name, file_path, first_rowid, last_rowid) VALUES (?1, ?2, ?3, ?4)";

const RANGES_BY_FILE: &str = "SELECT first_rowid, last_rowid FROM _dirsql_internal_ranges \
     WHERE table_name = ?1 AND file_path = ?2 ORDER BY first_rowid";

/// Name of the internal row-bookkeeping table.
///
/// Together with `_dirsql_internal_ranges` it is the sole record of row
/// ownership. A row inserted one at a time maps back to the file that
/// produced it here — `(table_name, file_path, row_index, rowid_ref)` — keyed
/// on the row's rowid; a batch inserted at once is owned as one
/// `(table_name, file_path, first_rowid, last_rowid)` range. Both are written
/// in the same SQLite transaction as the row insert/delete, so neither can
/// diverge from the rows it describes.
pub const INTERNAL_ROWS_TABLE: &str = "_dirsql_internal_rows";

/// Name of the internal batch-ownership table.
pub const INTERNAL_RANGES_TABLE: &str = "_dirsql_internal_ranges";

/// Create the internal row-ownership tables (`_dirsql_internal_rows` and
/// `_dirsql_internal_ranges`) and their by-file indexes if they don't already
/// exist. Idempotent.
///
/// **Real** tables (not virtual): they live in the persisted cache and are
/// written inside the same transaction as the row inserts/deletes they
/// describe, which is what gives ownership crash-atomicity with the user
/// rows.
pub fn ensure_internal_rows_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _dirsql_internal_rows (
            table_name TEXT NOT NULL,
            file_path  TEXT NOT NULL,
            row_index  INTEGER NOT NULL,
            rowid_ref  INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS _dirsql_internal_rows_by_file
            ON _dirsql_internal_rows(table_name, file_path);
         CREATE TABLE IF NOT EXISTS _dirsql_internal_ranges (
            table_name  TEXT NOT NULL,
            file_path   TEXT NOT NULL,
            first_rowid INTEGER NOT NULL,
            last_rowid  INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS _dirsql_internal_ranges_by_file
            ON _dirsql_internal_ranges(table_name, file_path);",
    )
}

/// A batch of rows laid out in a table's DDL column order, so each row binds
/// positionally with no per-cell lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedRows {
    pub columns: Vec<String>,
    pub cells: Vec<Vec<Value>>,
}

impl ShapedRows {
    /// The rows as column-name maps, for callers that compare rows by name.
    pub fn to_maps(&self) -> Vec<HashMap<String, Value>> {
        self.cells
            .iter()
            .map(|row| {
                self.columns
                    .iter()
                    .cloned()
                    .zip(row.iter().cloned())
                    .collect()
            })
            .collect()
    }
}

fn json_into_value(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::String(s) => Value::Text(s),
        other => crate::json_to_value(&other),
    }
}

/// One query's rows plus the column order the SELECT list asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<HashMap<String, Value>>,
}

pub struct Db {
    conn: Connection,
    /// Where this connection's path tables hold their rows for the statement in
    /// progress; `query_ordered` brackets each statement with it.
    scope: Arc<StatementScope>,
    /// The directory a path-table's glob is resolved against. `None` disables
    /// the path-table fallback entirely, leaving `query()` errors untouched.
    path_table_root: Option<PathBuf>,
    /// Whether a missing `files` table should carry the path-table hint. Only
    /// true for an index built with no config and no programmatic tables —
    /// exactly where `files` used to exist implicitly. A user who declared
    /// tables and forgot `files` gets the plain SQLite error.
    hint_legacy_files_table: bool,
    /// Configured skip rules a path-table scan applies on top of
    /// [`DEFAULT_IGNORES`].
    path_table_ignore: Vec<String>,
    /// Whether a path-table scan respects `.gitignore` files. On by default;
    /// the CLI's `--no-ignore` turns it off.
    path_table_gitignore: bool,
    /// When set (the CLI's `--on-file`), every path-table is minted over the
    /// parser instead of the stat columns: its rows and schema come from the
    /// command's output. `None` keeps the stat path-table behavior.
    path_table_parser: Option<String>,
    /// Counts the worker round trips a query pays for. Shared with every
    /// declared function registered on `conn`; inert until a function is both
    /// declared and called.
    calls: Arc<CallReporter>,
    /// The declared functions that batch. Every statement opens a collect
    /// pass on each of them; empty, a statement runs exactly once.
    batched: Vec<Arc<Worker>>,
}

/// Drops every batching worker's gathered replies when the statement that
/// gathered them is done, whichever way it left.
struct ClearOnDrop<'a>(&'a [Arc<Worker>]);

impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        for worker in self.0 {
            let _ = worker.clear();
        }
    }
}

/// A worker failure outside any SQL call, surfaced the way a failing call
/// would be.
fn worker_error(message: String) -> DbError {
    DbError::Sqlite(rusqlite::Error::UserFunctionError(message.into()))
}

impl Db {
    /// Open the default, ephemeral `Db`: an **anonymous disk-backed temp
    /// database** (`Connection::open("")`), not `:memory:`.
    ///
    /// SQLite creates a private temp file and deletes it immediately after
    /// opening, so the OS reclaims it even on a crash or SIGKILL, and there
    /// is never a name to collide on. Pages spill to disk as the index
    /// grows — only the page cache stays resident — so memory does not
    /// scale with the indexed corpus. The file lands in the directory
    /// SQLite's VFS picks (`SQLITE_TMPDIR` → `TMPDIR` → `/var/tmp` →
    /// `/usr/tmp` → `/tmp`); export `SQLITE_TMPDIR` to steer it off a
    /// tmpfs mount.
    pub fn new() -> Result<Self> {
        let conn = Connection::open("")?;
        // The database is a private temp file that dies with the process, so
        // durability buys nothing.
        conn.pragma_update(None, "synchronous", "OFF")?;
        ensure_internal_rows_table(&conn)?;
        let scope = StatementScope::new();
        vtab::load_module(&conn, Arc::clone(&scope))?;
        parsed_vtab::load_module(&conn, Arc::clone(&scope))?;
        Ok(Self {
            conn,
            scope,
            path_table_root: None,
            hint_legacy_files_table: false,
            path_table_ignore: Vec::new(),
            path_table_gitignore: true,
            path_table_parser: None,
            calls: CallReporter::new(),
            batched: Vec::new(),
        })
    }

    /// Open a `Db` backed by a named on-disk SQLite file. Used by the
    /// persistent cache path; the anonymous temp database is the default.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // Best-effort: WAL is unavailable on some filesystems (SQLite keeps the
        // prior journal mode and returns it); the cache still works there.
        let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        ensure_internal_rows_table(&conn)?;
        let scope = StatementScope::new();
        vtab::load_module(&conn, Arc::clone(&scope))?;
        parsed_vtab::load_module(&conn, Arc::clone(&scope))?;
        Ok(Self {
            conn,
            scope,
            path_table_root: None,
            hint_legacy_files_table: false,
            path_table_ignore: Vec::new(),
            path_table_gitignore: true,
            path_table_parser: None,
            calls: CallReporter::new(),
            batched: Vec::new(),
        })
    }

    /// Set the directory a path-table's glob resolves against — the index
    /// root, not this database's own file. Until it is set, `query()` reports
    /// an unknown table exactly as SQLite does.
    pub fn set_path_table_root(&mut self, root: PathBuf) {
        self.path_table_root = Some(root);
    }

    /// Arm the hint that redirects a missing `files` table to the path-table
    /// form. Set only for a configless, tableless index; see the field docs.
    pub fn set_hint_legacy_files_table(&mut self, hint: bool) {
        self.hint_legacy_files_table = hint;
    }

    /// Add the configured `ignore` patterns to the skip rules a path-table
    /// scan applies, on top of the built-in defaults.
    pub fn add_path_table_ignore(&mut self, patterns: Vec<String>) {
        self.path_table_ignore.extend(patterns);
    }

    /// Set whether a path-table scan respects `.gitignore` files. On by
    /// default; `false` is the CLI's `--no-ignore`. The built-in defaults and
    /// configured `ignore` patterns apply either way.
    pub fn set_path_table_gitignore(&mut self, gitignore: bool) {
        self.path_table_gitignore = gitignore;
    }

    /// Attach a parser command to every path-table minted on this connection
    /// (the CLI's `--on-file`). With it set, a path-table's rows and schema
    /// come from the command's output instead of the stat columns.
    pub fn set_path_table_parser(&mut self, command: String) {
        self.path_table_parser = Some(command);
    }

    /// Register the declared functions on the connection. Inert: no worker
    /// is spawned until a query calls one.
    pub(crate) fn register_functions(
        &mut self,
        functions: &[ResolvedFunction],
    ) -> rusqlite::Result<()> {
        let batched = functions::register_all(&self.conn, functions, &self.calls)?;
        self.batched.extend(batched);
        Ok(())
    }

    #[cfg(test)]
    fn register_batched_worker(&mut self, worker: Arc<Worker>, arities: &[u8]) {
        functions::register_worker(&self.conn, &worker, arities, true).unwrap();
        self.batched.push(worker);
    }

    /// Borrow the underlying SQLite connection. Internal use only — exposed
    /// to the `persist` module so it can manage the sidecar tables.
    #[doc(hidden)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Load a SQLite extension shared library onto this connection.
    ///
    /// `entrypoint` overrides the init symbol; when `None`, SQLite derives it
    /// from the filename. Extension loading is enabled for the duration of the
    /// call via [`rusqlite::LoadExtensionGuard`] and disabled again on return,
    /// so the SQL `load_extension()` function is never left exposed to later
    /// queries. A missing or unloadable file surfaces as [`DbError::Sqlite`].
    #[expect(
        unsafe_code,
        reason = "rusqlite's load_extension API is unsafe by design"
    )]
    pub fn load_extension(&self, path: &Path, entrypoint: Option<&str>) -> Result<()> {
        // SAFETY: loading an extension executes native code from `path`. The
        // path is operator-supplied configuration (a `[[dirsql.extension]]`
        // entry), at the same trust level as the DDL the operator already
        // controls. The guard enables loading on construction and disables it
        // on drop at the end of this block — after the load — so the SQL
        // `load_extension()` function is never left exposed to later queries.
        unsafe {
            let _guard = rusqlite::LoadExtensionGuard::new(&self.conn)?;
            self.conn.load_extension(path, entrypoint)?;
        }
        Ok(())
    }

    /// Run a table's user-provided DDL **batch**, executed **verbatim**:
    /// dirsql injects no tracking columns — row ownership lives entirely in
    /// `_dirsql_internal_rows`, so a table's schema is exactly the DDL the
    /// user wrote.
    ///
    /// The batch may hold any number of statements, of any kind: the row table
    /// plus whatever indexes, virtual tables and triggers go with it. It runs
    /// inside one transaction, so a statement SQLite rejects leaves nothing
    /// behind, and SQLite's own error is returned untouched.
    ///
    /// `name` is the table's declared name, validated as a safe unquoted SQL
    /// identifier before the batch reaches SQLite: it is spliced into
    /// `format!()`-built INSERT/DELETE SQL downstream, so a poisoned name must
    /// never get that far. What the batch actually created is settled
    /// afterwards against SQLite's catalog, not by reading the DDL text.
    pub fn create_table(&self, name: &str, ddl: &str) -> Result<()> {
        validate_identifier(name)?;
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute_batch(ddl)?;
        tx.commit()?;
        Ok(())
    }

    /// Return the column names declared in `table`'s DDL.
    pub fn get_table_columns(&self, table: &str) -> Result<Vec<String>> {
        validate_identifier(table)?;
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(columns)
    }

    /// Normalize a row to match the table schema.
    ///
    /// In relaxed mode (strict=false): extra keys are dropped, missing keys become NULL.
    /// In strict mode (strict=true): every row key is validated as a safe
    /// SQL identifier, then any extra or missing key produces a
    /// SchemaMismatch error. The identifier check runs *first* so a
    /// malformed key (e.g. one containing SQL syntax) reports as
    /// [`DbError::InvalidIdentifier`] rather than as a less-actionable
    /// "extra columns" mismatch.
    pub fn normalize_row(
        &self,
        table: &str,
        row: &HashMap<String, Value>,
        strict: bool,
    ) -> Result<HashMap<String, Value>> {
        let columns = self.get_table_columns(table)?;
        Self::normalize_against(table, &columns, row.clone(), strict)
    }

    /// [`Self::normalize_row`] over every row of one table, reading the
    /// table's columns once rather than once per row.
    pub fn normalize_rows(
        &self,
        table: &str,
        rows: Vec<HashMap<String, Value>>,
        strict: bool,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let columns = self.get_table_columns(table)?;
        rows.into_iter()
            .map(|row| Self::normalize_against(table, &columns, row, strict))
            .collect()
    }

    fn normalize_against(
        table: &str,
        columns: &[String],
        mut row: HashMap<String, Value>,
        strict: bool,
    ) -> Result<HashMap<String, Value>> {
        if strict {
            for key in row.keys() {
                validate_identifier(key)?;
            }
            let column_set: std::collections::HashSet<&str> =
                columns.iter().map(|s| s.as_str()).collect();
            let row_keys: std::collections::HashSet<&str> =
                row.keys().map(|s| s.as_str()).collect();
            let extra: Vec<&str> = row_keys.difference(&column_set).copied().collect();
            if !extra.is_empty() {
                return Err(DbError::SchemaMismatch(format!(
                    "extra columns not in table {}: {}",
                    table,
                    extra.join(", ")
                )));
            }
            let missing: Vec<&str> = column_set.difference(&row_keys).copied().collect();
            if !missing.is_empty() {
                return Err(DbError::SchemaMismatch(format!(
                    "missing columns for table {}: {}",
                    table,
                    missing.join(", ")
                )));
            }
            Ok(row)
        } else {
            let mut normalized = HashMap::with_capacity(columns.len());
            for col in columns {
                let value = row.remove(col).unwrap_or(Value::Null);
                normalized.insert(col.clone(), value);
            }
            Ok(normalized)
        }
    }

    /// Insert a row into a table.
    /// `row` contains user-defined columns only. `file_path` and `row_index` are tracked internally.
    ///
    /// Helper to execute insert_row statements on any connection.
    /// Called from insert_row() either inside a caller-supplied transaction or
    /// inside a transaction opened by insert_row().
    fn insert_row_stmts(
        conn: &Connection,
        sql: &str,
        params: &[&dyn rusqlite::types::ToSql],
        table: &str,
        file_path: &str,
        row_index: usize,
    ) -> Result<()> {
        conn.prepare_cached(sql)?.execute(params)?;
        let rowid = conn.last_insert_rowid();
        conn.prepare_cached(MAPPING_INSERT)?
            .execute(rusqlite::params![
                table,
                file_path,
                i64::try_from(row_index).expect("row index fits in i64"),
                rowid
            ])?;
        Ok(())
    }

    /// Lay `rows` out in `table`'s DDL column order.
    ///
    /// In relaxed mode (strict=false): a key the table lacks is dropped and a
    /// column the row lacks is NULL. In strict mode (strict=true): every key
    /// is validated as a safe SQL identifier, then any extra or missing key
    /// produces a SchemaMismatch error. The identifier check runs *first* so a
    /// malformed key reports as [`DbError::InvalidIdentifier`] rather than as
    /// a less-actionable "extra columns" mismatch. Every row is shaped before
    /// any is returned, so a hook that got one row wrong fails as a unit.
    pub fn shape_rows(&self, table: &str, rows: Vec<JsonRow>, strict: bool) -> Result<ShapedRows> {
        let columns = self.get_table_columns(table)?;
        let mut cells = Vec::with_capacity(rows.len());
        for row in rows {
            if strict {
                for (key, _) in &row.0 {
                    validate_identifier(key)?;
                }
                let extra: Vec<&str> = row
                    .0
                    .iter()
                    .map(|(key, _)| key.as_str())
                    .filter(|key| !columns.iter().any(|c| c == key))
                    .collect();
                if !extra.is_empty() {
                    return Err(DbError::SchemaMismatch(format!(
                        "extra columns not in table {}: {}",
                        table,
                        extra.join(", ")
                    )));
                }
                let missing: Vec<&str> = columns
                    .iter()
                    .map(String::as_str)
                    .filter(|c| !row.0.iter().any(|(key, _)| key == c))
                    .collect();
                if !missing.is_empty() {
                    return Err(DbError::SchemaMismatch(format!(
                        "missing columns for table {}: {}",
                        table,
                        missing.join(", ")
                    )));
                }
            }
            let in_order = row.0.len() == columns.len()
                && row.0.iter().zip(&columns).all(|((key, _), c)| key == c);
            if in_order {
                cells.push(row.0.into_iter().map(|(_, v)| json_into_value(v)).collect());
                continue;
            }
            let mut shaped = vec![Value::Null; columns.len()];
            for (key, value) in row.0 {
                if let Some(i) = columns.iter().position(|c| *c == key) {
                    shaped[i] = json_into_value(value);
                }
            }
            cells.push(shaped);
        }
        Ok(ShapedRows { columns, cells })
    }

    /// Insert every row of `rows` under `file_path`, in order, through one
    /// prepared statement, and own the batch as one rowid range rather than
    /// a mapping row each.
    ///
    /// The range spans the smallest to the largest rowid the batch received,
    /// so a table whose DDL aliases rowid (`INTEGER PRIMARY KEY`) must not
    /// interleave batches of different owners.
    pub fn insert_rows(&self, table: &str, rows: &ShapedRows, file_path: &str) -> Result<()> {
        validate_identifier(table)?;
        let columns = &rows.columns;
        let sql = if columns.is_empty() {
            format!("INSERT INTO \"{table}\" DEFAULT VALUES")
        } else {
            let names: Vec<String> = columns.iter().map(|c| format!("\"{c}\"")).collect();
            let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{i}")).collect();
            format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({})",
                names.join(", "),
                placeholders.join(", ")
            )
        };
        let tx = if self.conn.is_autocommit() {
            Some(self.conn.unchecked_transaction()?)
        } else {
            None
        };
        {
            let mut insert = self.conn.prepare_cached(&sql)?;
            let mut range: Option<(i64, i64)> = None;
            for row in &rows.cells {
                for (i, value) in row.iter().enumerate() {
                    insert.raw_bind_parameter(i + 1, value)?;
                }
                insert.raw_execute()?;
                let rowid = self.conn.last_insert_rowid();
                range = Some(match range {
                    None => (rowid, rowid),
                    Some((first, last)) => (first.min(rowid), last.max(rowid)),
                });
            }
            if let Some((first, last)) = range {
                self.conn
                    .prepare_cached(RANGE_INSERT)?
                    .execute(rusqlite::params![table, file_path, first, last])?;
            }
        }
        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(())
    }

    /// Both the table name and every user-provided column name are validated
    /// as safe SQL identifiers before being interpolated. A column key with
    /// SQL syntax (e.g. `id); DROP TABLE t; --`) produces a clean
    /// [`DbError::InvalidIdentifier`] rather than a cryptic SQLite parse
    /// failure.
    ///
    /// The user-row insert and its `_dirsql_internal_rows` mapping row commit
    /// in ONE transaction (either via a transaction opened here in autocommit
    /// mode, or via the caller's already-open transaction), so a crash between
    /// them can never leave a row without its mapping (or vice versa).
    pub fn insert_row(
        &self,
        table: &str,
        row: &HashMap<String, Value>,
        file_path: &str,
        row_index: usize,
    ) -> Result<()> {
        validate_identifier(table)?;
        for key in row.keys() {
            validate_identifier(key)?;
        }

        // The user row carries ONLY user columns; ownership is recorded in
        // `_dirsql_internal_rows`, keyed on the row's rowid. A column-less row
        // (SQLite requires ≥1 declared column, so this is only reachable
        // defensively) uses `DEFAULT VALUES`.
        // Sorted so every row of a table binds the same statement text, which
        // is what lets the prepared-statement cache serve it.
        let mut keys: Vec<&String> = row.keys().collect();
        keys.sort();
        let columns: Vec<String> = keys.iter().map(|c| format!("\"{c}\"")).collect();
        let sql = if columns.is_empty() {
            format!("INSERT INTO \"{table}\" DEFAULT VALUES")
        } else {
            let placeholders: Vec<String> =
                (1..=columns.len()).map(|i| format!("?{}", i)).collect();
            format!(
                "INSERT INTO \"{}\" ({}) VALUES ({})",
                table,
                columns.join(", "),
                placeholders.join(", "),
            )
        };

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = keys
            .iter()
            .map(|k| &row[*k] as &dyn rusqlite::types::ToSql)
            .collect();

        if self.conn.is_autocommit() {
            let tx = self.conn.unchecked_transaction()?;
            Self::insert_row_stmts(
                &tx,
                &sql,
                param_refs.as_slice(),
                table,
                file_path,
                row_index,
            )?;
            tx.commit()?;
        } else {
            Self::insert_row_stmts(
                &self.conn,
                &sql,
                param_refs.as_slice(),
                table,
                file_path,
                row_index,
            )?;
        }
        Ok(())
    }

    /// Read back the rows a given file produced for `table`: the rows it
    /// inserted one at a time, ordered by row index, then the batches it
    /// inserted at once, in rowid order. User columns are qualified with the
    /// table alias so a user column named like a mapping column stays
    /// unambiguous.
    ///
    /// A row read back compares equal to the normalized row that was inserted
    /// only when the on-file callback's value types match the declared column
    /// affinities (SQLite coerces on insert otherwise, e.g. `Integer(5)` into
    /// a TEXT column comes back `Text("5")`).
    pub fn get_rows_by_file(
        &self,
        table: &str,
        file_path: &str,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let user_columns = self.get_table_columns(table)?;
        let mut col_list = user_columns
            .iter()
            .map(|c| format!("t.\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        if col_list.is_empty() {
            // No columns can only happen for a nonexistent table (SQLite
            // requires ≥1 declared column); keep the SELECT list valid and
            // let SQLite report "no such table".
            col_list = "1".to_string();
        }
        let read_row = |row: &rusqlite::Row<'_>| {
            let mut map = HashMap::new();
            for (i, name) in user_columns.iter().enumerate() {
                let v: rusqlite::types::Value = row.get(i)?;
                map.insert(name.clone(), Value::from(v));
            }
            Ok(map)
        };
        let mapped = format!(
            "SELECT {col_list} FROM \"{table}\" AS t \
             JOIN _dirsql_internal_rows AS m ON m.rowid_ref = t.rowid \
             WHERE m.table_name = ?1 AND m.file_path = ?2 ORDER BY m.row_index"
        );
        let mut out = self
            .conn
            .prepare(&mapped)?
            .query_map(rusqlite::params![table, file_path], &read_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let ranged = format!(
            "SELECT {col_list} FROM \"{table}\" AS t \
             WHERE t.rowid BETWEEN ?1 AND ?2 ORDER BY t.rowid"
        );
        let mut stmt = self.conn.prepare(&ranged)?;
        for (first, last) in Self::ranges_by_file(&self.conn, table, file_path)? {
            out.extend(
                stmt.query_map([first, last], &read_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }
        Ok(out)
    }

    fn ranges_by_file(
        conn: &Connection,
        table: &str,
        file_path: &str,
    ) -> rusqlite::Result<Vec<(i64, i64)>> {
        conn.prepare_cached(RANGES_BY_FILE)?
            .query_map(rusqlite::params![table, file_path], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect()
    }

    /// Every distinct `(table_name, file_path)` whose file sits beneath the
    /// directory `dir`, ordered by table then path.
    pub fn files_under(&self, dir: &str) -> Result<Vec<(String, String)>> {
        let prefix = format!("{dir}/");
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT table_name, file_path FROM ( \
                SELECT table_name, file_path FROM _dirsql_internal_rows \
                UNION ALL \
                SELECT table_name, file_path FROM _dirsql_internal_ranges) \
             WHERE substr(file_path, 1, length(?1)) = ?1 \
             ORDER BY table_name, file_path",
        )?;
        let rows = stmt.query_map([prefix], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Helper to execute delete_rows_by_file statements on any connection.
    /// Called from delete_rows_by_file() either inside a caller-supplied
    /// transaction or inside a transaction opened by delete_rows_by_file().
    fn delete_rows_by_file_stmts(conn: &Connection, table: &str, file_path: &str) -> Result<usize> {
        let mapped = format!(
            "DELETE FROM \"{table}\" WHERE rowid IN \
             (SELECT rowid_ref FROM _dirsql_internal_rows \
              WHERE table_name = ?1 AND file_path = ?2)"
        );
        let mut count = conn.execute(&mapped, rusqlite::params![table, file_path])?;
        conn.execute(
            "DELETE FROM _dirsql_internal_rows WHERE table_name = ?1 AND file_path = ?2",
            rusqlite::params![table, file_path],
        )?;
        let ranged = format!("DELETE FROM \"{table}\" WHERE rowid BETWEEN ?1 AND ?2");
        for (first, last) in Self::ranges_by_file(conn, table, file_path)? {
            count += conn.execute(&ranged, [first, last])?;
        }
        conn.execute(
            "DELETE FROM _dirsql_internal_ranges WHERE table_name = ?1 AND file_path = ?2",
            rusqlite::params![table, file_path],
        )?;
        Ok(count)
    }

    /// Delete all rows that were produced by a given file path. Row ownership
    /// is resolved through the `_dirsql_internal_rows` mapping and the
    /// `_dirsql_internal_ranges` batches.
    ///
    /// The user-row deletes and the matching ownership deletes commit in ONE
    /// transaction (either via a transaction opened here in autocommit mode,
    /// or via the caller's already-open transaction), so the ownership never
    /// outlives the rows it describes.
    pub fn delete_rows_by_file(&self, table: &str, file_path: &str) -> Result<usize> {
        validate_identifier(table)?;
        if self.conn.is_autocommit() {
            let tx = self.conn.unchecked_transaction()?;
            let count = Self::delete_rows_by_file_stmts(&tx, table, file_path)?;
            tx.commit()?;
            Ok(count)
        } else {
            Self::delete_rows_by_file_stmts(&self.conn, table, file_path)
        }
    }

    /// Query the database, returning rows as a list of column-name -> value maps.
    ///
    /// Rejects any statement that SQLite itself classifies as a write
    /// (INSERT / UPDATE / DELETE / DROP / CREATE / ALTER / REPLACE / VACUUM /
    /// ANALYZE / …) via `sqlite3_stmt_readonly`, surfaced here as
    /// [`DbError::WriteForbidden`].
    ///
    /// Results are vanilla SQLite: dirsql injects no tracking columns, so a
    /// table's columns are exactly the user's DDL and `SELECT *` returns
    /// exactly those columns — no filtering.
    ///
    /// A SQLite authorizer makes dirsql's internal bookkeeping tables
    /// unreachable through this surface: any read (or schema `PRAGMA`)
    /// targeting the reserved `_dirsql_*` namespace is denied at prepare time,
    /// surfaced as [`DbError::Unauthorized`]. The authorizer is installed only
    /// around this `prepare` and cleared immediately after, so the engine's own
    /// internal writes (`insert_row`, delete-by-file, persist), which never go
    /// through `query()`, are unaffected.
    pub fn query(&self, sql: &str) -> Result<Vec<HashMap<String, Value>>> {
        self.query_ordered(sql).map(|result| result.rows)
    }

    /// [`query`](Self::query), plus the projection order. A row is a
    /// `HashMap`, so the order the SELECT list asked for survives only in
    /// [`QueryResult::columns`]; a renderer that re-derives its columns from
    /// row keys prints them alphabetically instead.
    pub fn query_ordered(&self, sql: &str) -> Result<QueryResult> {
        // Bracket the whole statement, prepare included: a worker-backed
        // function called on every row makes this the slowest thing dirsql
        // does, and the guard is what erases the counter before the caller
        // prints the rows.
        let _calls = self.calls.phase();
        let _scope = self.scope.enter();

        if self.batched.is_empty() {
            return self.run_statement(sql);
        }
        let _collected = ClearOnDrop(&self.batched);
        for worker in &self.batched {
            worker.begin_collect().map_err(worker_error)?;
        }
        let collected = self.run_statement(sql);
        let mut invoked = false;
        for worker in &self.batched {
            invoked |= worker.end_collect().map_err(worker_error)?;
        }
        if !invoked {
            return collected;
        }
        self.run_statement(sql)
    }

    /// [`query_ordered`](Self::query_ordered) folding each row's cells, in
    /// projection order, into the state `start` makes from the projection,
    /// as SQLite yields them rather than collected first.
    pub fn query_each<S>(
        &self,
        sql: &str,
        start: impl FnOnce(&[String]) -> S,
        mut on_row: impl FnMut(&mut S, &[Value]),
    ) -> Result<S> {
        if !self.batched.is_empty() {
            // A batched worker may force a second run, so the rows of the
            // first are not final until it ends.
            let result = self.query_ordered(sql)?;
            let mut state = start(&result.columns);
            for row in &result.rows {
                let cells: Vec<Value> = result
                    .columns
                    .iter()
                    .map(|column| row.get(column).cloned().unwrap_or(Value::Null))
                    .collect();
                on_row(&mut state, &cells);
            }
            return Ok(state);
        }
        let _calls = self.calls.phase();
        let _scope = self.scope.enter();
        let (mut stmt, column_names) = self.prepare_statement(sql)?;
        let mut state = start(&column_names);
        let mut rows = stmt.query([])?;
        let mut cells = Vec::with_capacity(column_names.len());
        while let Some(row) = rows.next()? {
            cells.clear();
            for i in 0..column_names.len() {
                let val: rusqlite::types::Value = row.get(i)?;
                cells.push(Value::from(val));
            }
            on_row(&mut state, &cells);
        }
        Ok(state)
    }

    /// Prepare and run one read-only statement, registering the path-tables
    /// it names along the way.
    fn run_statement(&self, sql: &str) -> Result<QueryResult> {
        let (mut stmt, column_names) = self.prepare_statement(sql)?;
        let rows = stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for (i, name) in column_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                map.insert(name.clone(), Value::from(val));
            }
            Ok(map)
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(QueryResult {
            columns: column_names,
            rows: results,
        })
    }

    /// Prepare one read-only statement, registering the path-tables it names
    /// along the way, with its column names.
    fn prepare_statement(&self, sql: &str) -> Result<(rusqlite::Statement<'_>, Vec<String>)> {
        // Each iteration must register a table no earlier iteration did; a
        // repeat means the fallback is not making progress, so the SQLite
        // error stands. That is what bounds the loop.
        let mut attempted: HashSet<String> = HashSet::new();

        let (stmt, reads) = loop {
            let error = match self.prepare_guarded(sql) {
                Ok(prepared) => break prepared,
                Err(e) => e,
            };
            let Some((name, table)) = self.path_table_for(&error)? else {
                return Err(self.hint_unquoted_path(error));
            };
            if !attempted.insert(name.clone()) {
                return Err(error);
            }
            self.create_path_table(&name, &table)?;
        };

        if !stmt.readonly() {
            return Err(DbError::WriteForbidden);
        }
        self.scope.warm(&reads);
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        Ok((stmt, column_names))
    }

    /// Prepare `sql` with the internal-table / ATTACH authorizer installed for
    /// exactly that one call, and name the tables it reads. The authorizer is
    /// installed and cleared around every attempt, so a re-prepare after
    /// registering a path-table is gated identically to the first.
    fn prepare_guarded(&self, sql: &str) -> Result<(rusqlite::Statement<'_>, HashSet<String>)> {
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        use std::sync::{Arc, Mutex};

        // The authorizer runs at prepare time and may deny for two distinct
        // reasons; it records which one here so the caught error can carry the
        // matching message (the closure must be `'static`, so it owns a clone).
        let denial: Arc<Mutex<Option<&'static str>>> = Arc::new(Mutex::new(None));
        let denial_cb = Arc::clone(&denial);
        let reads: Arc<Mutex<HashSet<String>>> = Arc::default();
        let reads_cb = Arc::clone(&reads);
        self.conn
            .authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Read { table_name, .. } if is_internal_table(table_name) => {
                    *denial_cb.lock().unwrap() = Some(INTERNAL_TABLE_DENIED_MSG);
                    Authorization::Deny
                }
                AuthAction::Read { table_name, .. } => {
                    reads_cb.lock().unwrap().insert(table_name.to_owned());
                    Authorization::Allow
                }
                AuthAction::Pragma {
                    pragma_value: Some(value),
                    ..
                } if is_internal_table(value) => {
                    *denial_cb.lock().unwrap() = Some(INTERNAL_TABLE_DENIED_MSG);
                    Authorization::Deny
                }
                // ATTACH/DETACH are the only effectful actions SQLite classifies
                // as read-only, so the `readonly()` gate can't catch them;
                // deny here before the file is ever created or opened.
                AuthAction::Attach { .. } | AuthAction::Detach { .. } => {
                    *denial_cb.lock().unwrap() = Some(ATTACH_DENIED_MSG);
                    Authorization::Deny
                }
                _ => Authorization::Allow,
            }));
        let prepared = self.conn.prepare(sql);
        // Clear the authorizer so it only ever gates this one user query; the
        // shared connection's internal write paths must not see it.
        self.conn
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);

        match prepared {
            Ok(stmt) => Ok((stmt, std::mem::take(&mut *reads.lock().unwrap()))),
            Err(e)
                if e.sqlite_error_code()
                    == Some(rusqlite::ErrorCode::AuthorizationForStatementDenied) =>
            {
                let msg = denial.lock().unwrap().unwrap_or(INTERNAL_TABLE_DENIED_MSG);
                Err(DbError::Unauthorized(msg.to_string()))
            }
            Err(e) => Err(DbError::Sqlite(e)),
        }
    }

    /// Decide whether `error` names a table a path-table can supply, yielding
    /// its name and the resolved scan. `Ok(None)` means the error is not ours
    /// and stands as SQLite reported it.
    fn path_table_for(&self, error: &DbError) -> Result<Option<(String, PathTable)>> {
        let Some(root) = self.path_table_root.as_ref() else {
            return Ok(None);
        };
        let DbError::Sqlite(sqlite_error) = error else {
            return Ok(None);
        };
        let message = sqlite_error.to_string();
        let Some(name) = missing_table_name(&message) else {
            return Ok(None);
        };

        match path_table::resolve(name, root, home_dir().as_deref(), &|p| p.is_dir()) {
            Resolution::Table(table) => Ok(Some((name.to_string(), table))),
            Resolution::Hint => Err(DbError::PathTable(bare_glob_hint(name))),
            Resolution::NoHome => Err(DbError::PathTable(no_home_path_table(name))),
            Resolution::NotAPath if self.hints_legacy_files_table(name) => {
                Err(DbError::PathTable(legacy_files_table_hint()))
            }
            Resolution::NotAPath => Ok(None),
        }
    }

    /// Append the quoting hint when `error` is a syntax error over a bare
    /// filesystem path. SQLite parses `./` as punctuation, so `FROM ./` dies in
    /// the parser and never reaches the `no such table` fallback the rest of
    /// this module rides on -- quoting the path is what gets it there.
    fn hint_unquoted_path(&self, error: DbError) -> DbError {
        match self.unquoted_path_hint_for(&error) {
            Some(hint) => DbError::PathTable(format!("{error}\n{hint}")),
            None => error,
        }
    }

    /// The hint `error` earns, or `None` when it is not a syntax error over a
    /// path. Without a root the fallback is off, so a quoted path would fail
    /// too and the hint would be a dead end.
    fn unquoted_path_hint_for(&self, error: &DbError) -> Option<String> {
        self.path_table_root.as_ref()?;
        let DbError::Sqlite(rusqlite::Error::SqlInputError { sql, offset, .. }) = error else {
            return None;
        };
        let offset = usize::try_from(*offset).ok()?;
        path_token_at(sql, offset).map(unquoted_path_hint)
    }

    fn hints_legacy_files_table(&self, name: &str) -> bool {
        self.hint_legacy_files_table && name == LEGACY_DEFAULT_TABLE
    }

    /// Mint the path-table `name` over `table`, out of band: the DDL runs
    /// straight on the connection rather than through [`query`](Self::query),
    /// which classifies `CREATE VIRTUAL TABLE` as a write and would reject it.
    fn create_path_table(&self, name: &str, table: &PathTable) -> Result<()> {
        let index_root = self
            .path_table_root
            .as_deref()
            .expect("a path-table resolves only under an index root");
        let ignore: Vec<String> = DEFAULT_IGNORES
            .into_iter()
            .map(str::to_string)
            .chain(self.path_table_ignore.iter().cloned())
            .collect();
        self.conn.execute_batch(&path_table_ddl(
            name,
            table,
            &ignore,
            self.path_table_gitignore,
            self.path_table_parser.as_deref(),
            index_root,
        ))?;
        Ok(())
    }
}

/// A value that can be stored in SQLite.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        match self {
            Value::Null => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Null)),
            Value::Integer(i) => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Integer(*i))),
            Value::Real(f) => Ok(ToSqlOutput::Owned(rusqlite::types::Value::Real(*f))),
            Value::Text(s) => Ok(ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes()))),
            Value::Blob(b) => Ok(ToSqlOutput::Borrowed(ValueRef::Blob(b))),
        }
    }
}

/// What SQLite's catalog says about one table in `main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CatalogEntry {
    /// `pragma_table_list`'s `type`: `table`, `virtual`, `view` or `shadow`.
    pub kind: String,
    /// Whether the table is declared `WITHOUT ROWID`.
    pub without_rowid: bool,
}

/// The catalog rows a table's `ddl` batch could be responsible for: real and
/// virtual tables in `main`, never the shadow tables a virtual table keeps
/// behind it, SQLite's own `sqlite_*` tables, or dirsql's `_dirsql_*`
/// bookkeeping.
///
/// Virtual tables come first, and that order is load-bearing for the cache
/// sweep: dropping a shadow table before its virtual table poisons the virtual
/// table's own drop even with `IF EXISTS` (probed against sqlite-vec 0.1.9).
/// `sqlite_master`'s creation order happens to get this right today; asking
/// the catalog for the type makes it guaranteed.
const USER_TABLES_SQL: &str = "SELECT name FROM pragma_table_list \
     WHERE schema = 'main' AND type IN ('table', 'virtual') \
     AND name NOT LIKE '_dirsql_%' AND name NOT LIKE 'sqlite_%' \
     ORDER BY type = 'virtual' DESC, name";

/// Every user table on `conn`, in an order that is safe to drop in.
pub(crate) fn user_table_names(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(USER_TABLES_SQL)?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(names)
}

/// What `conn`'s catalog holds under `name`, or `None` if it holds nothing.
/// `pragma_table_list` is the catalog API for exactly this question, so no
/// part of dirsql has to interpret DDL text to answer it.
pub(crate) fn table_catalog_entry(
    conn: &Connection,
    name: &str,
) -> rusqlite::Result<Option<CatalogEntry>> {
    use rusqlite::OptionalExtension;
    conn.query_row(
        "SELECT type, wr FROM pragma_table_list WHERE schema = 'main' AND name = ?1",
        rusqlite::params![name],
        |row| {
            Ok(CatalogEntry {
                kind: row.get(0)?,
                without_rowid: row.get::<_, i64>(1)? != 0,
            })
        },
    )
    .optional()
}

impl From<rusqlite::types::Value> for Value {
    fn from(v: rusqlite::types::Value) -> Self {
        match v {
            rusqlite::types::Value::Null => Value::Null,
            rusqlite::types::Value::Integer(i) => Value::Integer(i),
            rusqlite::types::Value::Real(f) => Value::Real(f),
            rusqlite::types::Value::Text(s) => Value::Text(s),
            rusqlite::types::Value::Blob(b) => Value::Blob(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_blob_values_bind_without_copying() {
        let text = Value::Text("abc".into());
        let blob = Value::Blob(vec![1, 2]);
        assert_eq!(
            text.to_sql().unwrap(),
            ToSqlOutput::Borrowed(ValueRef::Text(b"abc"))
        );
        assert_eq!(
            blob.to_sql().unwrap(),
            ToSqlOutput::Borrowed(ValueRef::Blob(&[1, 2]))
        );
    }

    #[test]
    fn create_table_from_ddl() {
        let db = Db::new().unwrap();
        db.create_table(
            "comments",
            "CREATE TABLE comments (id TEXT PRIMARY KEY, body TEXT, resolved INTEGER)",
        )
        .unwrap();

        let rows = db.query("SELECT * FROM comments").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn create_table_invalid_ddl_returns_error() {
        let db = Db::new().unwrap();
        let result = db.create_table("t", "NOT VALID SQL");
        assert!(result.is_err());
    }

    #[test]
    fn create_table_runs_ddl_verbatim_no_injected_columns() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("1".into()))]),
            "test.json",
            0,
        )
        .unwrap();

        assert_eq!(db.get_table_columns("t").unwrap(), vec!["id".to_string()]);
        let rows = db.query("SELECT * FROM t").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 1);
        assert!(rows[0].contains_key("id"));
        assert!(!rows[0].contains_key("_dirsql_file_path"));
        assert!(!rows[0].contains_key("_dirsql_row_index"));
    }

    #[test]
    fn query_ordered_reports_the_select_list_order() {
        let db = Db::new().unwrap();
        db.create_table("docs", "CREATE TABLE docs (title TEXT, draft INTEGER)")
            .unwrap();

        let result = db.query_ordered("SELECT draft, title FROM docs").unwrap();

        assert_eq!(
            result.columns,
            vec!["draft".to_string(), "title".to_string()]
        );
    }

    fn titled_docs(titles: &[&str]) -> Db {
        let db = Db::new().unwrap();
        db.create_table("docs", "CREATE TABLE docs (title TEXT)")
            .unwrap();
        for (i, title) in titles.iter().enumerate() {
            let row = HashMap::from([("title".into(), Value::Text((*title).into()))]);
            db.insert_row("docs", &row, &format!("docs/{title}.md"), i)
                .unwrap();
        }
        db
    }

    fn texts(result: &QueryResult, column: &str) -> Vec<Value> {
        result.rows.iter().map(|row| row[column].clone()).collect()
    }

    /// The statement runs twice: once to gather the function's arguments
    /// (one round trip, then one batched request for the rest) and once for
    /// real, served from the replies. The rows are the real ones.
    #[test]
    fn query_ordered_serves_a_batched_function_from_one_collect_pass() {
        let mut db = titled_docs(&["a", "b", "c"]);
        let (worker, sent) = functions::test_support::batched_worker(
            "up",
            8,
            Arc::clone(&db.calls),
            vec![
                r#"{"ok": "A"}"#,
                r#"{"results": [{"ok": "B"}, {"ok": "C"}]}"#,
            ],
        );
        db.register_batched_worker(worker, &[1]);

        let result = db
            .query_ordered("SELECT up(title) AS v FROM docs ORDER BY title")
            .unwrap();

        assert_eq!(
            texts(&result, "v"),
            [
                Value::Text("A".into()),
                Value::Text("B".into()),
                Value::Text("C".into())
            ]
        );
        assert_eq!(
            *sent.lock().unwrap(),
            [r#"{"call":["a"]}"#, r#"{"calls":[["b"],["c"]]}"#]
        );
    }

    fn folded(db: &Db, sql: &str) -> (Vec<String>, Vec<Vec<Value>>) {
        db.query_each(
            sql,
            |columns| (columns.to_vec(), Vec::new()),
            |(_, rows), cells| rows.push(cells.to_vec()),
        )
        .unwrap()
    }

    #[test]
    fn query_each_folds_each_row_in_projection_order() {
        let db = titled_docs(&["a", "b"]);

        let (columns, rows) = folded(&db, "SELECT 7 AS n, title FROM docs ORDER BY title");

        assert_eq!(columns, ["n", "title"]);
        assert_eq!(
            rows,
            [
                [Value::Integer(7), Value::Text("a".into())],
                [Value::Integer(7), Value::Text("b".into())]
            ]
        );
    }

    #[test]
    fn query_each_folds_the_rows_a_batched_function_serves() {
        let mut db = titled_docs(&["a", "b"]);
        let (worker, _) = functions::test_support::batched_worker(
            "up",
            8,
            Arc::clone(&db.calls),
            vec![r#"{"ok": "A"}"#, r#"{"results": [{"ok": "B"}]}"#],
        );
        db.register_batched_worker(worker, &[1]);

        let (columns, rows) = folded(&db, "SELECT title, up(title) AS v FROM docs ORDER BY title");

        assert_eq!(columns, ["title", "v"]);
        assert_eq!(
            rows,
            [
                [Value::Text("a".into()), Value::Text("A".into())],
                [Value::Text("b".into()), Value::Text("B".into())]
            ]
        );
    }

    #[test]
    fn query_each_refuses_a_write() {
        let db = titled_docs(&["a"]);

        let err = db
            .query_each("DELETE FROM docs", |_| (), |_, _| {})
            .unwrap_err();

        assert!(matches!(err, DbError::WriteForbidden));
    }

    /// A statement that never reaches the function asks the worker nothing
    /// and keeps its first run's rows.
    #[test]
    fn a_statement_that_never_calls_a_batched_function_asks_the_worker_nothing() {
        let mut db = titled_docs(&["a", "b"]);
        let (worker, sent) =
            functions::test_support::batched_worker("up", 8, Arc::clone(&db.calls), vec![]);
        db.register_batched_worker(worker, &[1]);

        let result = db
            .query_ordered("SELECT title FROM docs ORDER BY title")
            .unwrap();

        assert_eq!(
            texts(&result, "title"),
            [Value::Text("a".into()), Value::Text("b".into())]
        );
        assert!(sent.lock().unwrap().is_empty());
    }

    /// Declared functions become callable, and the ones that batch are the
    /// ones the query path collects over.
    #[test]
    fn register_functions_registers_each_and_keeps_the_batching_workers() {
        let mut db = Db::new().unwrap();
        let resolved = |name: &str, batch| ResolvedFunction {
            name: name.to_string(),
            args: vec![1],
            command: "worker".to_string(),
            deterministic: false,
            timeout: std::time::Duration::from_secs(1),
            batch,
            cwd: PathBuf::from("."),
        };

        db.register_functions(&[resolved("plain", None), resolved("bulk", Some(4))])
            .unwrap();

        assert!(db.conn.prepare("SELECT plain(1), bulk(1)").is_ok());
        assert_eq!(db.batched.len(), 1);
    }

    /// What a collect pass gathered is dropped as soon as its statement is
    /// done — a corpus of vectors must not sit in memory until the next
    /// query — whether the statement succeeded or failed.
    #[test]
    fn gathered_replies_are_dropped_when_the_statement_is_done() {
        let mut db = titled_docs(&["a", "b"]);
        let (worker, _) = functions::test_support::batched_worker(
            "up",
            8,
            Arc::clone(&db.calls),
            vec![
                r#"{"ok": "A"}"#,
                r#"{"results": [{"ok": "B"}]}"#,
                r#"{"ok": "A"}"#,
                r#"{"results": [{"err": "boom"}]}"#,
            ],
        );
        db.register_batched_worker(Arc::clone(&worker), &[1]);
        let sql = "SELECT up(title) AS v FROM docs ORDER BY title";

        db.query_ordered(sql).unwrap();
        assert_eq!(worker.gathered(), 0, "after a successful statement");

        db.query_ordered(sql).unwrap_err();
        assert_eq!(worker.gathered(), 0, "after a failed statement");
    }

    /// The replies gathered for one statement are not an answer to the next:
    /// a repeated query asks the worker again.
    #[test]
    fn gathered_replies_do_not_outlive_their_statement() {
        let mut db = titled_docs(&["a", "b"]);
        let (worker, sent) = functions::test_support::batched_worker(
            "up",
            8,
            Arc::clone(&db.calls),
            vec![
                r#"{"ok": "A"}"#,
                r#"{"results": [{"ok": "B"}]}"#,
                r#"{"ok": "A2"}"#,
                r#"{"results": [{"ok": "B2"}]}"#,
            ],
        );
        db.register_batched_worker(worker, &[1]);
        let sql = "SELECT up(title) AS v FROM docs ORDER BY title";

        db.query_ordered(sql).unwrap();
        let again = db.query_ordered(sql).unwrap();

        assert_eq!(
            texts(&again, "v"),
            [Value::Text("A2".into()), Value::Text("B2".into())]
        );
        assert_eq!(sent.lock().unwrap().len(), 4);
    }

    /// A reply the worker could not give fails the query the way a failing
    /// call does, naming the function.
    #[test]
    fn a_failed_batched_reply_fails_the_query() {
        let mut db = titled_docs(&["a", "b"]);
        let (worker, _) = functions::test_support::batched_worker(
            "up",
            8,
            Arc::clone(&db.calls),
            vec![r#"{"ok": "A"}"#, r#"{"err": "boom"}"#],
        );
        db.register_batched_worker(worker, &[1]);

        let err = db
            .query_ordered("SELECT up(title) AS v FROM docs ORDER BY title")
            .unwrap_err();

        assert!(err.to_string().contains("boom"), "got: {err}");
    }

    #[test]
    fn query_ordered_carries_the_same_rows_query_returns() {
        let db = Db::new().unwrap();
        db.create_table("docs", "CREATE TABLE docs (title TEXT, draft INTEGER)")
            .unwrap();
        let row = HashMap::from([
            ("title".into(), Value::Text("Hello".into())),
            ("draft".into(), Value::Integer(0)),
        ]);
        db.insert_row("docs", &row, "docs/hello.md", 0).unwrap();

        let result = db.query_ordered("SELECT title, draft FROM docs").unwrap();

        assert_eq!(
            result.rows,
            db.query("SELECT title, draft FROM docs").unwrap()
        );
        assert_eq!(result.rows[0]["title"], Value::Text("Hello".into()));
    }

    #[test]
    fn pragma_table_info_matches_user_ddl_exactly() {
        let db = Db::new().unwrap();
        db.create_table("posts", "CREATE TABLE posts (title TEXT, draft INTEGER)")
            .unwrap();
        assert_eq!(
            db.get_table_columns("posts").unwrap(),
            vec!["title".to_string(), "draft".to_string()]
        );
    }

    #[test]
    fn insert_and_query_rows() {
        let db = Db::new().unwrap();
        db.create_table("docs", "CREATE TABLE docs (title TEXT, draft INTEGER)")
            .unwrap();

        let row = HashMap::from([
            ("title".into(), Value::Text("Hello".into())),
            ("draft".into(), Value::Integer(0)),
        ]);
        db.insert_row("docs", &row, "docs/hello.md", 0).unwrap();

        let results = db.query("SELECT title, draft FROM docs").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], Value::Text("Hello".into()));
        assert_eq!(results[0]["draft"], Value::Integer(0));
    }

    #[test]
    fn insert_multiple_rows_from_same_file() {
        let db = Db::new().unwrap();
        db.create_table("events", "CREATE TABLE events (action TEXT, ts INTEGER)")
            .unwrap();

        for (i, action) in ["created", "resolved", "reopened"].iter().enumerate() {
            let row = HashMap::from([
                ("action".into(), Value::Text(action.to_string())),
                ("ts".into(), Value::Integer(i64::try_from(i).unwrap())),
            ]);
            db.insert_row("events", &row, "thread.jsonl", i).unwrap();
        }

        let results = db.query("SELECT action FROM events ORDER BY ts").unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["action"], Value::Text("created".into()));
        assert_eq!(results[2]["action"], Value::Text("reopened".into()));
    }

    #[test]
    fn delete_rows_by_file_path() {
        let db = Db::new().unwrap();
        db.create_table("comments", "CREATE TABLE comments (id TEXT, body TEXT)")
            .unwrap();

        for (i, (id, file)) in [("1", "a.jsonl"), ("2", "a.jsonl"), ("3", "b.jsonl")]
            .iter()
            .enumerate()
        {
            let row = HashMap::from([
                ("id".into(), Value::Text(id.to_string())),
                ("body".into(), Value::Text("text".into())),
            ]);
            db.insert_row("comments", &row, file, i).unwrap();
        }

        let deleted = db.delete_rows_by_file("comments", "a.jsonl").unwrap();
        assert_eq!(deleted, 2);

        let results = db.query("SELECT id FROM comments").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["id"], Value::Text("3".into()));
    }

    #[test]
    fn query_with_where_clause() {
        let db = Db::new().unwrap();
        db.create_table("items", "CREATE TABLE items (name TEXT, count INTEGER)")
            .unwrap();

        for (i, (name, count)) in [("apple", 5), ("banana", 0), ("cherry", 3)]
            .iter()
            .enumerate()
        {
            let row = HashMap::from([
                ("name".into(), Value::Text(name.to_string())),
                ("count".into(), Value::Integer(*count)),
            ]);
            db.insert_row("items", &row, "items.json", i).unwrap();
        }

        let results = db
            .query("SELECT name FROM items WHERE count > 0 ORDER BY name")
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["name"], Value::Text("apple".into()));
        assert_eq!(results[1]["name"], Value::Text("cherry".into()));
    }

    #[test]
    fn get_table_columns_returns_user_columns_only() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, count INTEGER)")
            .unwrap();
        let cols = db.get_table_columns("t").unwrap();
        assert!(cols.contains(&"name".to_string()));
        assert!(cols.contains(&"count".to_string()));
        assert!(!cols.iter().any(|c| c.starts_with("_dirsql_")));
    }

    #[test]
    fn normalize_row_relaxed_drops_extra_keys() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let row = HashMap::from([
            ("name".into(), Value::Text("apple".into())),
            ("color".into(), Value::Text("red".into())),
        ]);
        let normalized = db.normalize_row("t", &row, false).unwrap();
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized["name"], Value::Text("apple".into()));
        assert!(!normalized.contains_key("color"));
    }

    #[test]
    fn normalize_row_relaxed_fills_missing_with_null() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, color TEXT)")
            .unwrap();
        let row = HashMap::from([("name".into(), Value::Text("apple".into()))]);
        let normalized = db.normalize_row("t", &row, false).unwrap();
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized["name"], Value::Text("apple".into()));
        assert_eq!(normalized["color"], Value::Null);
    }

    #[test]
    fn normalize_row_strict_errors_on_extra_keys() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let row = HashMap::from([
            ("name".into(), Value::Text("apple".into())),
            ("color".into(), Value::Text("red".into())),
        ]);
        let result = db.normalize_row("t", &row, true);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("extra columns"));
    }

    #[test]
    fn normalize_row_strict_errors_on_missing_keys() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, color TEXT)")
            .unwrap();
        let row = HashMap::from([("name".into(), Value::Text("apple".into()))]);
        let result = db.normalize_row("t", &row, true);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("missing columns"));
    }

    #[test]
    fn normalize_row_strict_accepts_exact_match() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, color TEXT)")
            .unwrap();
        let row = HashMap::from([
            ("name".into(), Value::Text("apple".into())),
            ("color".into(), Value::Text("red".into())),
        ]);
        let normalized = db.normalize_row("t", &row, true).unwrap();
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn insert_and_query_real_value() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (price REAL)").unwrap();
        let row = HashMap::from([("price".into(), Value::Real(9.99))]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT price FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["price"], Value::Real(9.99));
    }

    #[test]
    fn insert_and_query_null_value() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let row = HashMap::from([("name".into(), Value::Null)]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT name FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], Value::Null);
    }

    #[test]
    fn insert_and_query_blob_value() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (data BLOB)").unwrap();
        let row = HashMap::from([("data".into(), Value::Blob(vec![0xFF, 0x00]))]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT data FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["data"], Value::Blob(vec![0xFF, 0x00]));
    }

    #[test]
    fn select_star_returns_only_user_columns() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();
        let results = db.query("SELECT * FROM t").unwrap();
        assert_eq!(results[0].len(), 1);
        assert!(results[0].contains_key("id"));
    }

    #[test]
    fn dirsql_columns_no_longer_exist_on_user_tables() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();

        let err = db.query("SELECT _dirsql_file_path FROM t").unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");
        assert!(err.to_string().contains("no such column"), "got: {err}");
    }

    #[test]
    fn is_internal_table_recognizes_the_reserved_namespace() {
        assert!(is_internal_table("_dirsql_internal_rows"));
        assert!(is_internal_table("_dirsql_files"));
        assert!(is_internal_table("_dirsql_meta"));
        assert!(is_internal_table("_dirsql_anything_future"));
    }

    #[test]
    fn is_internal_table_allows_user_tables_and_fs_columns() {
        assert!(!is_internal_table("items"));
        assert!(!is_internal_table("posts"));
        assert!(!is_internal_table("path"));
        assert!(!is_internal_table("_dirsq"));
        assert!(!is_internal_table("dirsql_internal_rows"));
    }

    #[test]
    fn query_denies_reading_internal_rows_table() {
        let db = Db::new().unwrap();
        let err = db.query("SELECT * FROM _dirsql_internal_rows").unwrap_err();
        assert!(matches!(err, DbError::Unauthorized(_)), "got: {err}");
        assert!(
            err.to_string().contains("not authorized"),
            "message should say not authorized, got: {err}"
        );
    }

    #[test]
    fn query_denies_pragma_targeting_internal_table() {
        let db = Db::new().unwrap();
        let err = db
            .query("PRAGMA table_info(_dirsql_internal_rows)")
            .unwrap_err();
        assert!(matches!(err, DbError::Unauthorized(_)), "got: {err}");
    }

    #[test]
    fn query_allows_pragma_on_a_user_table() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let rows = db.query("PRAGMA table_info(t)").unwrap();
        assert_eq!(rows.len(), 1, "expected one column row, got {rows:?}");
        assert_eq!(rows[0]["name"], Value::Text("id".into()));
    }

    #[test]
    fn query_authorizer_is_cleared_after_each_query() {
        // `delete_rows_by_file` reads `_dirsql_internal_rows` without routing
        // through query(), so a leaked authorizer would make it fail.
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();

        let _ = db.query("SELECT * FROM _dirsql_internal_rows").unwrap_err();
        let rows = db.query("SELECT id FROM t").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], Value::Text("1".into()));

        let deleted = db.delete_rows_by_file("t", "file.json").unwrap();
        assert_eq!(
            deleted, 1,
            "delete-by-file reads _dirsql_internal_rows internally; a leaked \
             authorizer would have denied that read"
        );
    }

    #[test]
    fn prepare_guarded_names_the_tables_a_statement_reads() {
        let db = Db::new().unwrap();
        db.conn
            .execute_batch("CREATE TABLE a(x); CREATE TABLE b(y)")
            .unwrap();

        let (_, reads) = db
            .prepare_guarded("SELECT count(*) FROM a JOIN b ON b.y = a.x")
            .unwrap();

        assert_eq!(reads, HashSet::from(["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn prepare_guarded_names_a_table_the_statement_takes_no_column_from() {
        let db = Db::new().unwrap();
        db.conn.execute_batch("CREATE TABLE a(x)").unwrap();

        let (_, reads) = db.prepare_guarded("SELECT count(*) FROM a").unwrap();

        assert_eq!(reads, HashSet::from(["a".to_string()]));
    }

    #[test]
    fn internal_table_denied_message_mentions_the_namespace() {
        assert!(INTERNAL_TABLE_DENIED_MSG.contains("not authorized"));
        assert!(INTERNAL_TABLE_DENIED_MSG.contains("_dirsql_"));
    }

    #[test]
    fn query_denies_attach() {
        let db = Db::new().unwrap();
        let err = db
            .query("ATTACH 'file:should-not-open?mode=ro' AS ext")
            .unwrap_err();
        assert!(matches!(err, DbError::Unauthorized(_)), "got: {err}");
        let msg = err.to_string();
        assert!(msg.contains("not authorized"), "got: {msg}");
        assert!(msg.contains("ATTACH"), "got: {msg}");
    }

    #[test]
    fn query_denies_detach() {
        let db = Db::new().unwrap();
        let err = db.query("DETACH ext").unwrap_err();
        assert!(matches!(err, DbError::Unauthorized(_)), "got: {err}");
        assert!(err.to_string().contains("not authorized"), "got: {err}");
    }

    #[test]
    fn attach_denied_message_mentions_attach_and_detach() {
        assert!(ATTACH_DENIED_MSG.contains("not authorized"));
        assert!(ATTACH_DENIED_MSG.contains("ATTACH"));
        assert!(ATTACH_DENIED_MSG.contains("DETACH"));
    }

    #[test]
    fn query_still_allows_select_after_attach_denial() {
        // The authorizer is cleared after each query, so a denied ATTACH must
        // not poison the next normal read.
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let _ = db.query("ATTACH 'x.db' AS ext").unwrap_err();
        let rows = db.query("SELECT * FROM t").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn query_invalid_sql_returns_error() {
        let db = Db::new().unwrap();
        let result = db.query("SELECT FROM nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn insert_into_nonexistent_table_returns_error() {
        let db = Db::new().unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        let result = db.insert_row("nonexistent", &row, "f.json", 0);
        assert!(result.is_err());
    }

    #[test]
    fn delete_from_nonexistent_table_returns_error() {
        let db = Db::new().unwrap();
        let result = db.delete_rows_by_file("nonexistent", "f.json");
        assert!(result.is_err());
    }

    #[test]
    fn get_table_columns_nonexistent_table_returns_empty() {
        let db = Db::new().unwrap();
        let cols = db.get_table_columns("nonexistent").unwrap();
        assert!(cols.is_empty());
    }

    #[test]
    fn db_error_display_messages() {
        let err = DbError::SchemaMismatch("test error".to_string());
        assert!(err.to_string().contains("Schema mismatch"));

        let err = DbError::InvalidIdentifier("a b".to_string());
        assert!(err.to_string().contains("invalid identifier"));
    }

    #[test]
    fn delete_rows_by_file_returns_zero_for_no_matching_rows() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "a.json", 0).unwrap();
        let deleted = db.delete_rows_by_file("t", "nonexistent.json").unwrap();
        assert_eq!(deleted, 0);
    }

    #[test]
    fn new_is_disk_backed_not_memory() {
        // An in-memory SQLite database is pinned to journal_mode=memory; a
        // file-backed one — including the anonymous temp database `Db::new`
        // opens — defaults to journal_mode=delete.
        let db = Db::new().unwrap();
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
    }

    #[test]
    fn new_skips_fsync() {
        let db = Db::new().unwrap();
        let pragma = |name: &str| -> i64 {
            db.conn()
                .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(pragma("synchronous"), 0);
    }

    #[test]
    fn get_rows_by_file_returns_rows_in_row_index_order() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        // Row indices are deliberately out of insertion order.
        let second = HashMap::from([("id".into(), Value::Text("second".into()))]);
        let first = HashMap::from([("id".into(), Value::Text("first".into()))]);
        db.insert_row("t", &second, "a.json", 1).unwrap();
        db.insert_row("t", &first, "a.json", 0).unwrap();

        let rows = db.get_rows_by_file("t", "a.json").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], Value::Text("first".into()));
        assert_eq!(rows[1]["id"], Value::Text("second".into()));
    }

    #[test]
    fn get_rows_by_file_scopes_by_file_and_table() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.create_table("u", "CREATE TABLE u (id TEXT)").unwrap();
        let t_row = HashMap::from([("id".into(), Value::Text("t-row".into()))]);
        let u_row = HashMap::from([("id".into(), Value::Text("u-row".into()))]);
        let other = HashMap::from([("id".into(), Value::Text("other-file".into()))]);
        db.insert_row("t", &t_row, "a.json", 0).unwrap();
        db.insert_row("u", &u_row, "a.json", 0).unwrap();
        db.insert_row("t", &other, "b.json", 0).unwrap();

        let rows = db.get_rows_by_file("t", "a.json").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], Value::Text("t-row".into()));

        let none = db.get_rows_by_file("u", "b.json").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn get_rows_by_file_round_trips_all_value_variants() {
        let db = Db::new().unwrap();
        db.create_table(
            "t",
            "CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n TEXT)",
        )
        .unwrap();
        let row = HashMap::from([
            ("i".to_string(), Value::Integer(42)),
            ("r".to_string(), Value::Real(1.5)),
            ("s".to_string(), Value::Text("hello".into())),
            ("b".to_string(), Value::Blob(vec![0, 1, 2])),
            ("n".to_string(), Value::Null),
        ]);
        db.insert_row("t", &row, "a.json", 0).unwrap();

        let rows = db.get_rows_by_file("t", "a.json").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], row);
    }

    #[test]
    fn get_rows_by_file_missing_table_returns_error() {
        let db = Db::new().unwrap();
        let err = db.get_rows_by_file("ghost", "a.json").unwrap_err();
        assert!(err.to_string().contains("no such table"), "got: {err}");
    }

    #[test]
    fn open_on_unopenable_path_returns_error() {
        // `Db` is not `Debug`; `.map(|_| ())` discards the Ok payload so
        // `unwrap_err` works.
        let err = Db::open(Path::new("/nonexistent-dir-xyz/sub/cache.db"))
            .map(|_| ())
            .unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");
    }

    #[test]
    fn create_table_rejects_an_unsafe_declared_name() {
        let db = Db::new().unwrap();
        let err = db
            .create_table("t; DROP TABLE u; --", "CREATE TABLE t (id TEXT)")
            .unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err}");
    }

    #[test]
    fn normalize_row_propagates_column_lookup_error() {
        let db = Db::new().unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        let err = db
            .normalize_row("bad name with spaces", &row, false)
            .unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err}");
    }

    #[test]
    fn validate_identifier_accepts_simple_names() {
        for name in ["t", "_t", "_dirsql_file_path", "Photos2024", "Snake_Case"] {
            validate_identifier(name).unwrap_or_else(|e| panic!("{name:?} rejected: {e}"));
        }
    }

    #[test]
    fn validate_identifier_rejects_empty() {
        assert!(matches!(
            validate_identifier(""),
            Err(DbError::InvalidIdentifier(_))
        ));
    }

    #[test]
    fn validate_identifier_rejects_sql_syntax() {
        for bad in [
            "foo;DROP",
            "foo bar",
            "foo)",
            "1leading_digit",
            "evil--",
            "\"quoted\"",
            "id; DROP TABLE t; --",
        ] {
            assert!(
                matches!(validate_identifier(bad), Err(DbError::InvalidIdentifier(_))),
                "expected rejection for: {bad:?}"
            );
        }
    }

    #[test]
    fn create_table_rejects_ddl_with_sql_syntax_in_name_slot() {
        let db = Db::new().unwrap();
        let err = db
            .create_table(
                "evil;DROP_TABLE--",
                "CREATE TABLE evil;DROP_TABLE--(id TEXT)",
            )
            .unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err:?}");
    }

    #[test]
    fn insert_row_rejects_column_name_with_sql_syntax() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id); DROP TABLE t; --".into(), Value::Text("x".into()))]);
        let err = db.insert_row("t", &row, "f.json", 0).unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err:?}");
    }

    #[test]
    fn insert_row_round_trips_reserved_word_column() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (path TEXT, \"order\" INTEGER)")
            .unwrap();
        let row = HashMap::from([
            ("path".into(), Value::Text("a".into())),
            ("order".into(), Value::Integer(7)),
        ]);
        db.insert_row("t", &row, "f.json", 0).unwrap();

        let rows = db.get_rows_by_file("t", "f.json").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["order"], Value::Integer(7));
    }

    #[test]
    fn load_extension_missing_file_errors() {
        let db = Db::new().unwrap();
        let err = db
            .load_extension(Path::new("/nonexistent/dirsql-no-such-ext.so"), None)
            .unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");
    }

    #[test]
    fn open_sets_wal_journal_mode_and_normal_synchronous() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Db::open(&dir.path().join("cache.db")).unwrap();

        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal", "Db::open must set journal_mode=WAL");

        let synchronous: i64 = db
            .conn()
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 1, "Db::open must set synchronous=NORMAL (1)");
    }

    /// Read the raw mapping rows for a table, ordered for stable assertions.
    fn mapping_rows(db: &Db, table: &str) -> Vec<(String, i64, i64)> {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT file_path, row_index, rowid_ref FROM _dirsql_internal_rows \
                 WHERE table_name = ?1 ORDER BY rowid_ref",
            )
            .unwrap();
        stmt.query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
    }

    #[test]
    fn ensure_internal_rows_table_is_idempotent() {
        let db = Db::new().unwrap();
        ensure_internal_rows_table(&db.conn).unwrap();
        ensure_internal_rows_table(&db.conn).unwrap();
        let table_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [INTERNAL_ROWS_TABLE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(table_count, 1);
        let index_count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='_dirsql_internal_rows_by_file'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(index_count, 1);
    }

    #[test]
    fn insert_row_records_mapping() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("a".into()))]),
            "f.jsonl",
            3,
        )
        .unwrap();

        let rows = mapping_rows(&db, "t");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "f.jsonl");
        assert_eq!(rows[0].1, 3);
        let user_rowid: i64 = db
            .conn
            .query_row("SELECT rowid FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows[0].2, user_rowid);
    }

    #[test]
    fn insert_row_captures_user_declared_rowid_alias() {
        // A user-declared `INTEGER PRIMARY KEY` is a rowid alias: the
        // inserted value becomes the rowid.
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        db.insert_row(
            "t",
            &HashMap::from([
                ("id".into(), Value::Integer(42)),
                ("name".into(), Value::Text("x".into())),
            ]),
            "f.json",
            0,
        )
        .unwrap();

        let rows = mapping_rows(&db, "t");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].2, 42, "rowid_ref must be the user-declared rowid");
    }

    #[test]
    fn files_under_lists_each_file_beneath_the_directory_once_across_tables() {
        let db = Db::new().unwrap();
        db.create_table("t1", "CREATE TABLE t1 (id TEXT)").unwrap();
        db.create_table("t2", "CREATE TABLE t2 (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("x".into()))]);
        for (table, file, idx) in [
            ("t1", "dir/a.jsonl", 0),
            ("t1", "dir/a.jsonl", 1),
            ("t1", "dir/sub/b.jsonl", 0),
            ("t2", "dir/c.jsonl", 0),
            ("t1", "dir2/x.jsonl", 0),
            ("t1", "other.jsonl", 0),
        ] {
            db.insert_row(table, &row, file, idx).unwrap();
        }

        assert_eq!(
            db.files_under("dir").unwrap(),
            vec![
                ("t1".to_string(), "dir/a.jsonl".to_string()),
                ("t1".to_string(), "dir/sub/b.jsonl".to_string()),
                ("t2".to_string(), "dir/c.jsonl".to_string()),
            ],
            "a multi-row file is listed once; `dir2/` is not beneath `dir`"
        );
        assert!(db.files_under("nothing").unwrap().is_empty());
    }

    #[test]
    fn files_under_surfaces_sql_failure() {
        let db = Db::new().unwrap();
        db.conn
            .execute("DROP TABLE _dirsql_internal_rows", [])
            .unwrap();
        let err = db.files_under("dir").unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err:?}");
    }

    #[test]
    fn delete_rows_by_file_removes_mapping() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        for (i, file) in ["a.jsonl", "a.jsonl", "b.jsonl"].iter().enumerate() {
            db.insert_row(
                "t",
                &HashMap::from([("id".into(), Value::Text(i.to_string()))]),
                file,
                i,
            )
            .unwrap();
        }
        db.delete_rows_by_file("t", "a.jsonl").unwrap();

        let rows = mapping_rows(&db, "t");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "b.jsonl");
    }

    #[test]
    fn delete_rows_by_file_resolves_ownership_through_mapping() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("a".into()))]),
            "real.json",
            0,
        )
        .unwrap();

        assert_eq!(db.delete_rows_by_file("t", "absent.json").unwrap(), 0);
        assert_eq!(db.delete_rows_by_file("t", "real.json").unwrap(), 1);
        let remaining: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn delete_rows_by_file_is_scoped_to_its_table() {
        // Rows in different tables can share a rowid; the delete must not
        // cross tables for the same file path.
        let db = Db::new().unwrap();
        db.create_table("t1", "CREATE TABLE t1 (id TEXT)").unwrap();
        db.create_table("t2", "CREATE TABLE t2 (id TEXT)").unwrap();
        db.insert_row(
            "t1",
            &HashMap::from([("id".into(), Value::Text("x".into()))]),
            "shared.json",
            0,
        )
        .unwrap();
        db.insert_row(
            "t2",
            &HashMap::from([("id".into(), Value::Text("y".into()))]),
            "shared.json",
            0,
        )
        .unwrap();

        assert_eq!(db.delete_rows_by_file("t1", "shared.json").unwrap(), 1);
        let t2_rows: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM t2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(t2_rows, 1, "t2's row for the same path must be untouched");
    }

    #[test]
    fn failed_row_insert_leaves_no_mapping_row() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT UNIQUE)")
            .unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("dup".into()))]),
            "a.json",
            0,
        )
        .unwrap();
        let err = db
            .insert_row(
                "t",
                &HashMap::from([("id".into(), Value::Text("dup".into()))]),
                "b.json",
                0,
            )
            .unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");

        let rows = mapping_rows(&db, "t");
        assert_eq!(
            rows.len(),
            1,
            "the failed insert must not add a mapping row"
        );
        assert_eq!(rows[0].0, "a.json");
    }

    #[test]
    fn failed_mapping_insert_rolls_back_row_insert() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        // Remove the mapping table so the second statement in the transaction
        // errors with "no such table".
        db.conn
            .execute("DROP TABLE _dirsql_internal_rows", [])
            .unwrap();
        let err = db
            .insert_row(
                "t",
                &HashMap::from([("id".into(), Value::Text("x".into()))]),
                "a.json",
                0,
            )
            .unwrap_err();
        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "the user row must have rolled back");
    }

    #[test]
    fn create_table_runs_every_statement_in_the_batch() {
        let db = Db::new().unwrap();
        db.create_table(
            "notes",
            "CREATE TABLE notes (path TEXT);\n\
             CREATE INDEX notes_path ON notes(path);\n\
             CREATE VIRTUAL TABLE notes_fts USING fts5(path);",
        )
        .unwrap();

        assert_eq!(user_table_names(&db.conn).unwrap(), ["notes_fts", "notes"]);
        let index: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_list('notes') WHERE name = 'notes_path'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(index, 1, "the batch's CREATE INDEX must have run");
    }

    #[test]
    fn create_table_rolls_back_a_batch_sqlite_rejects() {
        let db = Db::new().unwrap();
        let err = db
            .create_table(
                "notes",
                "CREATE TABLE notes (path TEXT); CREATE TABLE oops (",
            )
            .unwrap_err();

        assert!(matches!(err, DbError::Sqlite(_)), "got: {err}");
        assert!(
            err.to_string().contains("incomplete input"),
            "SQLite's own error must come through untouched, got: {err}"
        );
        assert!(
            user_table_names(&db.conn).unwrap().is_empty(),
            "a rejected batch must leave none of its earlier statements behind"
        );
    }

    #[test]
    fn create_table_validates_the_name_before_running_the_batch() {
        let db = Db::new().unwrap();
        let err = db
            .create_table("a b", "CREATE TABLE \"a b\" (path TEXT)")
            .unwrap_err();

        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err}");
        assert!(
            user_table_names(&db.conn).unwrap().is_empty(),
            "a poisoned name must be rejected before SQLite sees the batch"
        );
    }

    #[test]
    fn create_table_still_runs_a_without_rowid_ddl() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT PRIMARY KEY) WITHOUT ROWID")
            .unwrap();
        let rows = db.query("SELECT * FROM t").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn user_table_names_lists_virtual_tables_before_real_ones() {
        let db = Db::new().unwrap();
        // Created real-table-first, so `sqlite_master`'s creation order would
        // put `notes` ahead of the virtual table it belongs to.
        db.conn
            .execute_batch(
                "CREATE TABLE notes (body TEXT);\n\
                 CREATE VIRTUAL TABLE notes_fts USING fts5(body);\n\
                 CREATE TABLE archive (body TEXT);",
            )
            .unwrap();

        assert_eq!(
            user_table_names(&db.conn).unwrap(),
            ["notes_fts", "archive", "notes"]
        );
    }

    #[test]
    fn user_table_names_omits_shadow_and_bookkeeping_tables() {
        let db = Db::new().unwrap();
        db.conn
            .execute_batch("CREATE VIRTUAL TABLE notes_fts USING fts5(body);")
            .unwrap();

        let names = user_table_names(&db.conn).unwrap();
        assert_eq!(
            names,
            ["notes_fts"],
            "FTS5's shadow tables and `_dirsql_internal_rows` must not be listed"
        );
    }

    #[test]
    fn table_catalog_entry_reports_kind_and_without_rowid() {
        let db = Db::new().unwrap();
        db.conn
            .execute_batch(
                "CREATE TABLE plain (id TEXT);\n\
                 CREATE TABLE keyed (id TEXT PRIMARY KEY) WITHOUT ROWID;\n\
                 CREATE VIRTUAL TABLE virt USING fts5(body);",
            )
            .unwrap();

        let plain = table_catalog_entry(&db.conn, "plain").unwrap().unwrap();
        assert_eq!(plain.kind, "table");
        assert!(!plain.without_rowid);

        let keyed = table_catalog_entry(&db.conn, "keyed").unwrap().unwrap();
        assert_eq!(keyed.kind, "table");
        assert!(keyed.without_rowid);

        let virt = table_catalog_entry(&db.conn, "virt").unwrap().unwrap();
        assert_eq!(virt.kind, "virtual");

        assert_eq!(table_catalog_entry(&db.conn, "absent").unwrap(), None);
    }

    #[test]
    fn insert_row_joins_callers_open_transaction() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let tx = db.conn.unchecked_transaction().unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("a".into()))]),
            "a.json",
            0,
        )
        .unwrap();
        drop(tx); // rollback

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "row must have rolled back");
    }

    #[test]
    fn insert_row_inside_committed_transaction_persists() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let tx = db.conn.unchecked_transaction().unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("a".into()))]),
            "a.json",
            0,
        )
        .unwrap();
        tx.commit().unwrap();

        let rows = db.get_rows_by_file("t", "a.json").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("id").unwrap(), &Value::Text("a".into()));
    }

    fn range_rows(db: &Db, table: &str) -> Vec<(String, i64, i64)> {
        let mut stmt = db
            .conn
            .prepare(
                "SELECT file_path, first_rowid, last_rowid FROM _dirsql_internal_ranges \
                 WHERE table_name = ?1 ORDER BY first_rowid",
            )
            .unwrap();
        stmt.query_map([table], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn json_rows(payload: serde_json::Value) -> Vec<JsonRow> {
        let serde_json::Value::Array(rows) = payload else {
            panic!("payload must be an array");
        };
        rows.into_iter()
            .map(|row| {
                let serde_json::Value::Object(map) = row else {
                    panic!("row must be an object");
                };
                JsonRow(map.into_iter().collect())
            })
            .collect()
    }

    fn shaped(db: &Db, table: &str, payload: serde_json::Value) -> ShapedRows {
        db.shape_rows(table, json_rows(payload), false).unwrap()
    }

    #[test]
    fn shape_rows_lays_cells_out_in_ddl_order_whatever_the_key_order() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT, n INTEGER)")
            .unwrap();
        let out = shaped(
            &db,
            "t",
            serde_json::json!([{"id":"a","n":1},{"n":2,"id":"b"}]),
        );
        assert_eq!(out.columns, vec!["id", "n"]);
        assert_eq!(
            out.cells,
            vec![
                vec![Value::Text("a".into()), Value::Integer(1)],
                vec![Value::Text("b".into()), Value::Integer(2)],
            ]
        );
    }

    #[test]
    fn shape_rows_relaxed_drops_extra_keys_and_nulls_missing_columns() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, n INTEGER)")
            .unwrap();
        let out = shaped(&db, "t", serde_json::json!([{"name":"a"},{"n":1,"x":null}])).to_maps();
        assert_eq!(out[0]["name"], Value::Text("a".into()));
        assert_eq!(out[0]["n"], Value::Null);
        assert_eq!(out[1]["name"], Value::Null);
        assert_eq!(out[1]["n"], Value::Integer(1));
        assert!(!out[1].contains_key("x"));
    }

    #[test]
    fn shape_rows_strict_accepts_every_column_in_any_order() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT, n INTEGER)")
            .unwrap();
        let out = db
            .shape_rows("t", json_rows(serde_json::json!([{"n":1,"id":"a"}])), true)
            .unwrap();
        assert_eq!(
            out.cells,
            vec![vec![Value::Text("a".into()), Value::Integer(1)]]
        );
    }

    #[test]
    fn shape_rows_strict_names_the_extra_columns() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let err = db
            .shape_rows(
                "t",
                json_rows(serde_json::json!([{"name":"ok"},{"name":"x","color":"red"}])),
                true,
            )
            .unwrap_err();
        assert!(
            matches!(&err, DbError::SchemaMismatch(m) if m == "extra columns not in table t: color"),
            "got: {err}"
        );
    }

    #[test]
    fn shape_rows_strict_names_the_missing_columns() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, n INTEGER)")
            .unwrap();
        let err = db
            .shape_rows("t", json_rows(serde_json::json!([{"name":"ok"}])), true)
            .unwrap_err();
        assert!(
            matches!(&err, DbError::SchemaMismatch(m) if m == "missing columns for table t: n"),
            "got: {err}"
        );
    }

    #[test]
    fn shape_rows_strict_rejects_an_unsafe_key_before_judging_the_mismatch() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let err = db
            .shape_rows(
                "t",
                json_rows(serde_json::json!([{"name); DROP TABLE t; --":"x"}])),
                true,
            )
            .unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err}");
        let relaxed = db
            .shape_rows(
                "t",
                json_rows(serde_json::json!([{"name); DROP TABLE t; --":"x"}])),
                false,
            )
            .unwrap();
        assert_eq!(relaxed.cells, vec![vec![Value::Null]]);
    }

    #[test]
    fn insert_rows_owns_the_batch_as_one_rowid_range() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT, n INTEGER)")
            .unwrap();
        let rows = shaped(
            &db,
            "t",
            serde_json::json!([{"id":"a"},{"id":"b","n":2,"extra":9}]),
        );
        db.insert_rows("t", &rows, "x.json").unwrap();

        let read = db.get_rows_by_file("t", "x.json").unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0]["id"], Value::Text("a".into()));
        assert_eq!(read[0]["n"], Value::Null);
        assert_eq!(read[1]["n"], Value::Integer(2));
        assert!(!read[1].contains_key("extra"));
        assert!(mapping_rows(&db, "t").is_empty());
        assert_eq!(range_rows(&db, "t"), vec![("x.json".to_string(), 1, 2)]);
    }

    #[test]
    fn insert_rows_records_no_range_for_an_empty_batch() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_rows("t", &shaped(&db, "t", serde_json::json!([])), "x.json")
            .unwrap();
        assert!(range_rows(&db, "t").is_empty());
        assert!(db.get_rows_by_file("t", "x.json").unwrap().is_empty());
    }

    #[test]
    fn insert_rows_range_spans_a_user_declared_rowid_alias() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
            .unwrap();
        let rows = shaped(
            &db,
            "t",
            serde_json::json!([{"id":7,"name":"g"},{"id":3,"name":"c"}]),
        );
        db.insert_rows("t", &rows, "x.json").unwrap();

        assert_eq!(range_rows(&db, "t"), vec![("x.json".to_string(), 3, 7)]);
        let names: Vec<Value> = db
            .get_rows_by_file("t", "x.json")
            .unwrap()
            .into_iter()
            .map(|r| r["name"].clone())
            .collect();
        assert_eq!(
            names,
            vec![Value::Text("c".into()), Value::Text("g".into())]
        );
        assert_eq!(db.delete_rows_by_file("t", "x.json").unwrap(), 2);
    }

    #[test]
    fn insert_rows_binds_an_empty_column_list_by_default_values() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let rows = ShapedRows {
            columns: vec![],
            cells: vec![vec![], vec![]],
        };
        db.insert_rows("t", &rows, "x.json").unwrap();
        assert_eq!(db.get_rows_by_file("t", "x.json").unwrap().len(), 2);
    }

    #[test]
    fn get_rows_by_file_reads_mapped_rows_before_ranged_rows() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"r1"},{"id":"r2"}])),
            "x.json",
        )
        .unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("m".into()))]),
            "x.json",
            0,
        )
        .unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"other"}])),
            "y.json",
        )
        .unwrap();

        let ids: Vec<Value> = db
            .get_rows_by_file("t", "x.json")
            .unwrap()
            .into_iter()
            .map(|r| r["id"].clone())
            .collect();
        assert_eq!(
            ids,
            vec![
                Value::Text("m".into()),
                Value::Text("r1".into()),
                Value::Text("r2".into())
            ]
        );
    }

    #[test]
    fn delete_rows_by_file_removes_a_range_with_its_rows_and_leaves_other_owners() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"a"},{"id":"b"}])),
            "x.json",
        )
        .unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"c"}])),
            "y.json",
        )
        .unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("m".into()))]),
            "x.json",
            0,
        )
        .unwrap();

        assert_eq!(db.delete_rows_by_file("t", "x.json").unwrap(), 3);

        assert!(db.get_rows_by_file("t", "x.json").unwrap().is_empty());
        assert!(mapping_rows(&db, "t").is_empty());
        assert_eq!(range_rows(&db, "t"), vec![("y.json".to_string(), 3, 3)]);
        let left: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn files_under_lists_range_owners_too() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"a"}])),
            "dir/a.json",
        )
        .unwrap();
        db.insert_rows(
            "t",
            &shaped(&db, "t", serde_json::json!([{"id":"b"}])),
            "dir/a.json",
        )
        .unwrap();
        db.insert_rows("t", &shaped(&db, "t", serde_json::json!([{"id":"c"}])), "")
            .unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("m".into()))]),
            "dir/b.json",
            0,
        )
        .unwrap();

        assert_eq!(
            db.files_under("dir").unwrap(),
            vec![
                ("t".to_string(), "dir/a.json".to_string()),
                ("t".to_string(), "dir/b.json".to_string()),
            ]
        );
    }

    #[test]
    fn insert_rows_joins_callers_open_transaction() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        let rows = shaped(&db, "t", serde_json::json!([{"id":"a"}]));

        let tx = db.conn.unchecked_transaction().unwrap();
        db.insert_rows("t", &rows, "a.json").unwrap();
        drop(tx);

        assert!(db.get_rows_by_file("t", "a.json").unwrap().is_empty());
        assert!(range_rows(&db, "t").is_empty());
    }

    #[test]
    fn insert_rows_rejects_an_unsafe_table_name() {
        let db = Db::new().unwrap();
        let rows = ShapedRows {
            columns: vec![],
            cells: vec![],
        };
        let err = db.insert_rows("bad name", &rows, "a.json").unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err}");
    }

    #[test]
    fn normalize_rows_fails_as_a_unit_on_a_strict_mismatch() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT)").unwrap();
        let rows = vec![
            HashMap::from([("name".into(), Value::Text("ok".into()))]),
            HashMap::from([("color".into(), Value::Text("red".into()))]),
        ];
        let err = db.normalize_rows("t", rows, true).unwrap_err();
        assert!(matches!(err, DbError::SchemaMismatch(_)), "got: {err}");
    }

    #[test]
    fn normalize_rows_relaxed_shapes_every_row_to_the_table() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (name TEXT, n INTEGER)")
            .unwrap();
        let rows = vec![
            HashMap::from([("name".into(), Value::Text("a".into()))]),
            HashMap::from([("n".into(), Value::Integer(1)), ("x".into(), Value::Null)]),
        ];
        let out = db.normalize_rows("t", rows, false).unwrap();
        assert_eq!(out[0]["n"], Value::Null);
        assert_eq!(out[1]["name"], Value::Null);
        assert!(!out[1].contains_key("x"));
    }

    #[test]
    fn delete_rows_by_file_joins_callers_open_transaction() {
        let db = Db::new().unwrap();
        db.create_table("t", "CREATE TABLE t (id TEXT)").unwrap();
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("x".into()))]),
            "a.json",
            0,
        )
        .unwrap();

        let tx = db.conn.unchecked_transaction().unwrap();
        let count = db.delete_rows_by_file("t", "a.json").unwrap();
        assert_eq!(count, 1);
        drop(tx); // rollback

        let rows = db.get_rows_by_file("t", "a.json").unwrap();
        assert_eq!(rows.len(), 1, "delete must have rolled back");
    }

    #[test]
    fn missing_table_name_extracts_the_name() {
        assert_eq!(
            missing_table_name("no such table: ./docs/*.md"),
            Some("./docs/*.md")
        );
    }

    #[test]
    fn missing_table_name_ignores_other_errors() {
        assert_eq!(missing_table_name("no such column: x"), None);
    }

    #[test]
    fn missing_table_name_ignores_an_empty_name() {
        assert_eq!(missing_table_name("no such table: "), None);
    }

    #[test]
    fn path_token_at_recovers_a_path_from_its_first_character() {
        assert_eq!(
            path_token_at("SELECT * FROM ./docs/a.md", 14),
            Some("./docs/a.md")
        );
    }

    #[test]
    fn path_token_at_widens_left_when_sqlite_points_mid_path() {
        assert_eq!(
            path_token_at("SELECT * FROM src/main.rs", 17),
            Some("src/main.rs")
        );
    }

    #[test]
    fn path_token_at_stops_at_the_first_non_path_character() {
        assert_eq!(path_token_at("SELECT * FROM a/b, c", 15), Some("a/b"));
    }

    #[test]
    fn path_token_at_ignores_a_token_with_no_slash() {
        assert_eq!(path_token_at("SELECT * FROM 1nvalid", 14), None);
    }

    #[test]
    fn path_token_at_ignores_an_offset_inside_a_character() {
        // `é` is two bytes, so offset 15 splits it; slicing there would panic.
        assert_eq!(path_token_at("SELECT * FROM é/x", 15), None);
    }

    #[test]
    fn unquoted_path_hint_names_the_quoted_form() {
        assert_eq!(
            unquoted_path_hint("./"),
            r#"hint: paths used as table names must be quoted; did you mean "./"?"#
        );
    }

    #[test]
    fn query_hints_at_quoting_for_an_unquoted_path() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db.query("SELECT * FROM ./").unwrap_err().to_string();

        assert!(err.contains("syntax error"), "got: {err}");
        assert!(err.contains(r#"did you mean "./"?"#), "got: {err}");
    }

    #[test]
    fn query_leaves_an_unquoted_path_unhinted_without_a_root() {
        let db = Db::new().unwrap();

        let err = db.query("SELECT * FROM ./").unwrap_err().to_string();

        assert!(err.contains("syntax error"), "got: {err}");
        assert!(
            !err.contains("did you mean"),
            "quoting leads nowhere without a root, got: {err}"
        );
    }

    #[test]
    fn query_leaves_a_pathless_syntax_error_unhinted() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db.query("SELECT * FROM 1nvalid").unwrap_err().to_string();

        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn query_leaves_an_offsetless_syntax_error_unhinted() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        // "incomplete input" names no token, so there is no offset to widen from.
        let err = db.query("SELECT * FROM").unwrap_err().to_string();

        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn bare_glob_hint_names_the_dot_slash_form() {
        assert_eq!(
            bare_glob_hint("**/*.md"),
            "no such table: **/*.md; did you mean './**/*.md'?"
        );
    }

    #[test]
    fn no_home_path_table_names_the_table() {
        let msg = no_home_path_table("~/notes");
        assert!(msg.contains("~/notes"), "got: {msg}");
        assert!(
            msg.contains("HOME"),
            "the fix must be actionable, got: {msg}"
        );
    }

    fn docs_path_table() -> PathTable {
        PathTable {
            root: PathBuf::from("/root"),
            glob: "docs/*.md".to_string(),
            path_prefix: String::new(),
        }
    }

    #[test]
    fn path_table_ddl_creates_in_temp_if_not_exists() {
        let ddl = path_table_ddl(
            "./docs/*.md",
            &docs_path_table(),
            &[],
            true,
            None,
            Path::new("/root"),
        );
        assert_eq!(
            ddl,
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.\"./docs/*.md\" \
             USING dirsql_path('/root', 'docs/*.md', '', 'gitignore')"
        );
    }

    #[test]
    fn path_table_ddl_passes_the_path_prefix() {
        let table = PathTable {
            root: PathBuf::from("/var/log"),
            glob: "*.log".to_string(),
            path_prefix: "/var/log".to_string(),
        };
        assert!(
            path_table_ddl(
                "/var/log/*.log",
                &table,
                &[],
                true,
                None,
                Path::new("/root"),
            )
            .contains("'/var/log', '*.log', '/var/log', 'gitignore-root')"),
            "got: {}",
            path_table_ddl(
                "/var/log/*.log",
                &table,
                &[],
                true,
                None,
                Path::new("/root"),
            )
        );
    }

    #[test]
    fn path_table_ddl_checks_the_root_of_a_wildcard_under_a_literal_prefix() {
        let table = PathTable {
            root: PathBuf::from("/root/dist"),
            glob: "*.js".to_string(),
            path_prefix: "dist".to_string(),
        };
        let ddl = path_table_ddl("./dist/*.js", &table, &[], true, None, Path::new("/root"));
        assert!(ddl.ends_with("'dist', 'gitignore-root')"), "got: {ddl}");
    }

    #[test]
    fn path_table_ddl_spares_the_root_of_a_literally_named_directory() {
        let table = PathTable {
            root: PathBuf::from("/root/dist"),
            glob: "*".to_string(),
            path_prefix: "dist".to_string(),
        };
        let ddl = path_table_ddl("./dist", &table, &[], true, None, Path::new("/root"));
        assert!(ddl.ends_with("'dist', 'gitignore')"), "got: {ddl}");
    }

    #[test]
    fn path_table_ddl_carries_the_no_gitignore_switch() {
        let ddl = path_table_ddl(
            "./",
            &docs_path_table(),
            &[],
            false,
            None,
            Path::new("/root"),
        );
        assert!(
            ddl.ends_with("'', 'no-gitignore')"),
            "gitignore off must emit the no-gitignore switch, got: {ddl}"
        );
    }

    #[test]
    fn path_table_ddl_appends_every_ignore_pattern() {
        let ddl = path_table_ddl(
            "./",
            &docs_path_table(),
            &["node_modules/**".to_string(), "*.tmp".to_string()],
            true,
            None,
            Path::new("/root"),
        );
        assert!(
            ddl.ends_with("'', 'gitignore', 'node_modules/**', '*.tmp')"),
            "got: {ddl}"
        );
    }

    #[test]
    fn path_table_ddl_uses_the_parsed_module_when_a_parser_is_set() {
        let ddl = path_table_ddl(
            "./docs/*.md",
            &docs_path_table(),
            &[],
            true,
            Some("cat"),
            Path::new("/root"),
        );
        assert_eq!(
            ddl,
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.\"./docs/*.md\" \
             USING dirsql_parsed('/root', 'docs/*.md', 'cat', 'gitignore', '/root')"
        );
    }

    #[test]
    fn path_table_ddl_parser_form_omits_the_path_prefix_and_carries_ignore() {
        let table = PathTable {
            root: PathBuf::from("/var/log"),
            glob: "*.log".to_string(),
            path_prefix: "/var/log".to_string(),
        };
        let ddl = path_table_ddl(
            "/var/log/*.log",
            &table,
            &["node_modules/**".to_string()],
            true,
            Some("parse.py"),
            Path::new("/index"),
        );
        assert!(
            ddl.ends_with(
                "USING dirsql_parsed('/var/log', '*.log', 'parse.py', 'gitignore-root', \
                 '/index', 'node_modules/**')"
            ),
            "the parser form drops the path prefix, names the index root and keeps \
             ignore rules, got: {ddl}"
        );
    }

    #[test]
    fn path_table_ddl_parser_form_quotes_a_command_with_a_quote() {
        let ddl = path_table_ddl(
            "./",
            &docs_path_table(),
            &[],
            true,
            Some("sh -c 'echo hi'"),
            Path::new("/root"),
        );
        assert!(
            ddl.contains("'sh -c ''echo hi'''"),
            "an embedded quote must be doubled, got: {ddl}"
        );
    }

    #[test]
    fn set_path_table_parser_arms_the_parsed_module() {
        let mut db = Db::new().unwrap();
        db.set_path_table_parser("cat".to_string());
        assert_eq!(db.path_table_parser.as_deref(), Some("cat"));
    }

    #[test]
    fn path_table_gitignore_defaults_on() {
        let db = Db::new().unwrap();
        assert!(db.path_table_gitignore);
    }

    #[test]
    fn set_path_table_gitignore_turns_the_switch_off() {
        let mut db = Db::new().unwrap();
        db.set_path_table_gitignore(false);
        assert!(!db.path_table_gitignore);
    }

    #[test]
    fn query_leaves_a_path_name_alone_without_a_root() {
        let db = Db::new().unwrap();

        let err = db.query("SELECT * FROM './'").unwrap_err().to_string();

        assert!(
            err.contains("no such table: ./"),
            "the fallback is off until a root is set, got: {err}"
        );
        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn query_resolves_a_path_table_once_a_root_is_set() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let rows = db.query("SELECT path FROM './' LIMIT 1").unwrap();

        assert!(rows.is_empty(), "an empty root yields no rows: {rows:?}");
    }

    #[test]
    fn query_hints_at_the_dot_slash_form_for_a_bare_glob() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db.query("SELECT * FROM '*.md'").unwrap_err().to_string();

        assert!(err.contains("did you mean './*.md'?"), "got: {err}");
    }

    #[test]
    fn query_resolves_an_absolute_path_table() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let rows = db
            .query("SELECT path FROM '/nonexistent-dirsql-dir/*.log' LIMIT 1")
            .unwrap();

        assert!(
            rows.is_empty(),
            "a missing directory yields no rows: {rows:?}"
        );
    }

    #[test]
    fn add_path_table_ignore_accumulates_configured_rules() {
        let mut db = Db::new().unwrap();
        db.add_path_table_ignore(vec!["*.tmp".to_string()]);
        db.add_path_table_ignore(vec!["*.bak".to_string()]);

        assert_eq!(db.path_table_ignore, ["*.tmp", "*.bak"]);
    }

    #[test]
    fn query_leaves_a_plain_typo_unchanged() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db.query("SELECT * FROM usrs").unwrap_err().to_string();

        assert!(err.contains("no such table: usrs"), "got: {err}");
        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn legacy_files_table_hint_names_files_and_the_dot_slash_form() {
        assert_eq!(
            legacy_files_table_hint(),
            "no such table: files; did you mean FROM './**'?"
        );
    }

    #[test]
    fn query_hints_at_the_dot_slash_form_for_a_missing_files_table() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));
        db.set_hint_legacy_files_table(true);

        let err = db.query("SELECT * FROM files").unwrap_err().to_string();

        assert!(err.contains("no such table: files"), "got: {err}");
        assert!(err.contains("did you mean FROM './**'?"), "got: {err}");
    }

    #[test]
    fn query_leaves_a_missing_files_table_unhinted_when_not_configless() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db.query("SELECT * FROM files").unwrap_err().to_string();

        assert!(err.contains("no such table: files"), "got: {err}");
        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn the_files_hint_is_scoped_to_that_exact_name() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));
        db.set_hint_legacy_files_table(true);

        let err = db.query("SELECT * FROM fyles").unwrap_err().to_string();

        assert!(err.contains("no such table: fyles"), "got: {err}");
        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn set_hint_legacy_files_table_can_be_disarmed() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));
        db.set_hint_legacy_files_table(true);
        db.set_hint_legacy_files_table(false);

        let err = db.query("SELECT * FROM files").unwrap_err().to_string();

        assert!(!err.contains("did you mean"), "got: {err}");
    }

    #[test]
    fn query_still_denies_internal_tables_after_the_loop_restructure() {
        let mut db = Db::new().unwrap();
        db.set_path_table_root(PathBuf::from("/nonexistent-dirsql-root"));

        let err = db
            .query("SELECT * FROM _dirsql_internal_rows")
            .unwrap_err()
            .to_string();

        assert!(err.contains("not authorized"), "got: {err}");
    }
}
