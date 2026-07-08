use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("DDL parse error: {0}")]
    DdlParse(String),

    #[error("invalid identifier: {0:?} (must match [A-Za-z_][A-Za-z0-9_]*)")]
    InvalidIdentifier(String),

    #[error(
        "query() only accepts read-only statements; SQLite classified this statement as a write"
    )]
    WriteForbidden,

    #[error("{0}")]
    Unauthorized(String),
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

/// Name of the internal row-bookkeeping table.
///
/// The sole record of row ownership: it maps every inserted user row back to
/// the file that produced it — `(table_name, file_path, row_index, rowid_ref)`
/// — keyed on the row's rowid. Written in the same SQLite transaction as each
/// row insert/delete, so it can never diverge from the rows it describes.
pub const INTERNAL_ROWS_TABLE: &str = "_dirsql_internal_rows";

/// Create the internal `_dirsql_internal_rows` bookkeeping table and its
/// by-file index if they don't already exist. Idempotent.
///
/// A **real** table (not virtual): it lives in the persisted cache and is
/// written inside the same transaction as the row inserts/deletes it
/// describes, which is what gives the mapping crash-atomicity with the user
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
            ON _dirsql_internal_rows(table_name, file_path);",
    )
}

pub struct Db {
    conn: Connection,
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
        ensure_internal_rows_table(&conn)?;
        Ok(Self { conn })
    }

    /// Open a `Db` backed by a named on-disk SQLite file. Used by the
    /// persistent cache path; the anonymous temp database is the default.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        ensure_internal_rows_table(&conn)?;
        Ok(Self { conn })
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

    /// Create a table from a user-provided DDL statement, executed
    /// **verbatim**: dirsql injects no tracking columns — row ownership lives
    /// entirely in `_dirsql_internal_rows`, so a table's schema is exactly
    /// the DDL the user wrote.
    ///
    /// Validates that the parsed table name is a safe unquoted SQL identifier
    /// before handing the DDL to SQLite, so a DDL like
    /// `CREATE TABLE foo;DROP_TABLE_bar--(id TEXT)` can't yield a poisoned
    /// table name that breaks downstream `format!()`-built SQL.
    pub fn create_table(&self, ddl: &str) -> Result<()> {
        // A dirsql table is a per-file row table; an extension-backed virtual
        // table is not one, so reject `CREATE VIRTUAL TABLE` with a clear
        // message. Load the extension and use its functions in queries instead.
        if is_virtual_table_ddl(ddl) {
            return Err(DbError::DdlParse(
                "CREATE VIRTUAL TABLE is not supported as a dirsql table \
                 (dirsql tables are per-file row tables); load the extension \
                 and call its functions in queries instead"
                    .to_string(),
            ));
        }
        let table = parse_table_name(ddl).ok_or_else(|| DbError::DdlParse(ddl.to_string()))?;
        validate_identifier(&table)?;
        // A `WITHOUT ROWID` table has no rowid, so `last_insert_rowid()` cannot
        // identify the inserted row and the `_dirsql_internal_rows.rowid_ref`
        // mapping would be meaningless. Warn; the table is still created.
        if is_without_rowid_ddl(ddl) {
            eprintln!(
                "dirsql: table `{table}` is declared WITHOUT ROWID; internal row \
                 bookkeeping relies on rowid and WITHOUT ROWID tables will be \
                 rejected in a future release"
            );
        }
        self.conn.execute(ddl, [])?;
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
        let column_set: std::collections::HashSet<&str> =
            columns.iter().map(|s| s.as_str()).collect();
        let row_keys: std::collections::HashSet<&str> = row.keys().map(|s| s.as_str()).collect();

        if strict {
            for key in row.keys() {
                validate_identifier(key)?;
            }
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
            Ok(row.clone())
        } else {
            let mut normalized = HashMap::new();
            for col in &columns {
                let value = row.get(col).cloned().unwrap_or(Value::Null);
                normalized.insert(col.clone(), value);
            }
            Ok(normalized)
        }
    }

    /// Insert a row into a table.
    /// `row` contains user-defined columns only. `file_path` and `row_index` are tracked internally.
    ///
    /// Both the table name and every user-provided column name are validated
    /// as safe SQL identifiers before being interpolated. A column key with
    /// SQL syntax (e.g. `id); DROP TABLE t; --`) produces a clean
    /// [`DbError::InvalidIdentifier`] rather than a cryptic SQLite parse
    /// failure.
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
        let columns: Vec<String> = row.keys().map(|c| format!("\"{c}\"")).collect();
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

        let params: Vec<Box<dyn rusqlite::types::ToSql>> = row
            .values()
            .map(|v| Box::new(v.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        // The user-row insert and its `_dirsql_internal_rows` mapping row commit
        // in ONE transaction, so a crash between them can never leave a row
        // without its mapping (or vice versa). `last_insert_rowid()` is read
        // *after* the user insert and *before* the mapping insert, so it
        // captures the user row's rowid (including a user-declared `INTEGER
        // PRIMARY KEY` rowid alias).
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(&sql, param_refs.as_slice())?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO _dirsql_internal_rows (table_name, file_path, row_index, rowid_ref) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![table, file_path, row_index as i64, rowid],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Read back the rows a given file produced for `table`, ordered by row
    /// index. Ownership and ordering come from the `_dirsql_internal_rows`
    /// mapping (joined on `rowid`); user columns are qualified with the table
    /// alias so a user column named like a mapping column stays unambiguous.
    ///
    /// A row read back compares equal to the normalized row that was inserted
    /// only when the extract's value types match the declared column
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
        let sql = format!(
            "SELECT {col_list} FROM \"{table}\" AS t \
             JOIN _dirsql_internal_rows AS m ON m.rowid_ref = t.rowid \
             WHERE m.table_name = ?1 AND m.file_path = ?2 ORDER BY m.row_index"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![table, file_path], |row| {
            let mut map = HashMap::new();
            for (i, name) in user_columns.iter().enumerate() {
                let v: rusqlite::types::Value = row.get(i)?;
                map.insert(name.clone(), Value::from(v));
            }
            Ok(map)
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete all rows that were produced by a given file path. Row ownership
    /// is resolved through the `_dirsql_internal_rows` mapping.
    ///
    /// The user-row deletes and the matching mapping deletes commit in ONE
    /// transaction, so the mapping never outlives the rows it describes.
    pub fn delete_rows_by_file(&self, table: &str, file_path: &str) -> Result<usize> {
        validate_identifier(table)?;
        let tx = self.conn.unchecked_transaction()?;
        let sql = format!(
            "DELETE FROM {} WHERE rowid IN \
             (SELECT rowid_ref FROM _dirsql_internal_rows \
              WHERE table_name = ?1 AND file_path = ?2)",
            table
        );
        let count = tx.execute(&sql, rusqlite::params![table, file_path])?;
        tx.execute(
            "DELETE FROM _dirsql_internal_rows WHERE table_name = ?1 AND file_path = ?2",
            rusqlite::params![table, file_path],
        )?;
        tx.commit()?;
        Ok(count)
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
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        use std::sync::{Arc, Mutex};

        // The authorizer runs at prepare time and may deny for two distinct
        // reasons; it records which one here so the caught error can carry the
        // matching message (the closure must be `'static`, so it owns a clone).
        let denial: Arc<Mutex<Option<&'static str>>> = Arc::new(Mutex::new(None));
        let denial_cb = Arc::clone(&denial);
        self.conn
            .authorizer(Some(move |ctx: AuthContext<'_>| match ctx.action {
                AuthAction::Read { table_name, .. } if is_internal_table(table_name) => {
                    *denial_cb.lock().unwrap() = Some(INTERNAL_TABLE_DENIED_MSG);
                    Authorization::Deny
                }
                AuthAction::Pragma {
                    pragma_value: Some(value),
                    ..
                } if is_internal_table(value) => {
                    *denial_cb.lock().unwrap() = Some(INTERNAL_TABLE_DENIED_MSG);
                    Authorization::Deny
                }
                // ATTACH/DETACH are the only effectful actions SQLite classifies
                // as read-only, so the `readonly()` gate below can't catch them;
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

        let mut stmt = match prepared {
            Ok(stmt) => stmt,
            Err(e)
                if e.sqlite_error_code()
                    == Some(rusqlite::ErrorCode::AuthorizationForStatementDenied) =>
            {
                let msg = denial.lock().unwrap().unwrap_or(INTERNAL_TABLE_DENIED_MSG);
                return Err(DbError::Unauthorized(msg.to_string()));
            }
            Err(e) => return Err(DbError::Sqlite(e)),
        };
        if !stmt.readonly() {
            return Err(DbError::WriteForbidden);
        }
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

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
        Ok(results)
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

impl rusqlite::types::ToSql for Value {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            Value::Null => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Null,
            )),
            Value::Integer(i) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Integer(*i),
            )),
            Value::Real(f) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Real(*f),
            )),
            Value::Text(s) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Text(s.clone()),
            )),
            Value::Blob(b) => Ok(rusqlite::types::ToSqlOutput::Owned(
                rusqlite::types::Value::Blob(b.clone()),
            )),
        }
    }
}

/// Extract the table name from a `CREATE TABLE` DDL statement.
///
/// The name token in `CREATE TABLE [IF NOT EXISTS] <name> (...)` may be:
///   - a bare identifier:    `comments`
///   - a quoted identifier:  `"comments"`, `` `comments` ``, `[comments]`
///   - schema-qualified:     `main.comments`, `"main"."comments"`
///
/// Surrounding quotes are SQL *delimiters*, not part of the name, so they are
/// stripped: a tool that emits `CREATE TABLE "comments" (...)` (as ORMs and
/// schema generators routinely do) names the table `comments`. Schema-qualified
/// names resolve to the table segment. Returns `None` when the input isn't a
/// `CREATE TABLE` or carries no name token.
///
/// Deliberately a small, pure tokenizer rather than a full SQL parser:
/// dirsql constrains table names to safe unquoted identifiers via
/// [`validate_identifier`], so the handful of forms above are the only ones
/// that can actually resolve to a usable table.
/// Strip leading whitespace and SQL comments (`-- ...` to end-of-line and
/// `/* ... */` blocks, repeated) from `s`. An unterminated block comment
/// consumes the rest of the input. Every returned slice starts on a char
/// boundary, so callers can index it safely.
fn skip_ws_comments(s: &str) -> &str {
    let mut s = s;
    loop {
        let trimmed = s.trim_start();
        if let Some(after) = trimmed.strip_prefix("--") {
            match after.find('\n') {
                Some(i) => s = &after[i + 1..],
                None => return "",
            }
        } else if let Some(after) = trimmed.strip_prefix("/*") {
            match after.find("*/") {
                Some(i) => s = &after[i + 2..],
                None => return "",
            }
        } else {
            return trimmed;
        }
    }
}

/// If `s` begins with the ASCII keyword `kw` (case-insensitive) followed by a
/// non-identifier boundary, return the remainder after it; otherwise `None`.
/// The boundary check keeps a longer identifier (`TABLES`, `iffy`) from
/// matching a keyword prefix.
fn strip_keyword_ci<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let bytes = s.as_bytes();
    if bytes.len() < kw.len() || !bytes[..kw.len()].eq_ignore_ascii_case(kw.as_bytes()) {
        return None;
    }
    let rest = &s[kw.len()..];
    match rest.chars().next() {
        Some(c) if c.is_alphanumeric() || c == '_' => None,
        _ => Some(rest),
    }
}

pub fn parse_table_name(ddl: &str) -> Option<String> {
    // Match `CREATE TABLE` only as the *statement head*: skip leading
    // whitespace and line/block comments and scan the original `ddl` by byte
    // offset (never a `to_uppercase()` copy, whose length can differ from the
    // source and mis-slice on Unicode). A `-- CREATE TABLE ...` comment must
    // not hijack the name.
    let rest = skip_ws_comments(ddl);
    let rest = strip_keyword_ci(rest, "CREATE")?;
    let rest = strip_keyword_ci(skip_ws_comments(rest), "TABLE")?;
    let mut rest = skip_ws_comments(rest);

    // Skip optional "IF NOT EXISTS". The keyword boundary check in
    // `strip_keyword_ci` keeps a table literally named e.g. `iffy` from being
    // mistaken for `IF`.
    if let Some(after_if) = strip_keyword_ci(rest, "IF") {
        let after_not = strip_keyword_ci(skip_ws_comments(after_if), "NOT")?;
        let after_exists = strip_keyword_ci(skip_ws_comments(after_not), "EXISTS")?;
        rest = skip_ws_comments(after_exists);
    }

    // `schema.table`: keep the last dot-separated segment. Each segment may
    // be quoted independently.
    let mut chars = rest.chars().peekable();
    let mut table = parse_identifier(&mut chars)?;
    while chars.peek() == Some(&'.') {
        chars.next();
        table = parse_identifier(&mut chars)?;
    }

    if table.is_empty() { None } else { Some(table) }
}

/// Read one identifier segment from `chars`: a quoted identifier
/// (`"..."`, `` `...` ``, or `[...]`, with the doubled-delimiter escape for the
/// matching close on the SQL-standard quote forms) or a bare run up to the
/// first whitespace, `(`, or `.`. Returns the *unquoted* text. Returns `None`
/// only when the cursor is already exhausted.
fn parse_identifier(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    let (open, close) = match *chars.peek()? {
        '"' => ('"', '"'),
        '`' => ('`', '`'),
        '[' => ('[', ']'),
        _ => {
            let mut bare = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '(' || c == '.' {
                    break;
                }
                bare.push(c);
                chars.next();
            }
            return Some(bare);
        }
    };

    chars.next();
    let mut name = String::new();
    while let Some(c) = chars.next() {
        if c == close {
            // On `"` / `` ` ``, a doubled close delimiter is an escaped literal
            // (`""` -> `"`); SQLite's `[...]` form has no such escape.
            if open != '[' && chars.peek() == Some(&close) {
                chars.next();
                name.push(close);
                continue;
            }
            break;
        }
        name.push(c);
    }
    Some(name)
}

/// True if `ddl` is a `CREATE VIRTUAL TABLE` statement. dirsql tables are
/// per-file row tables, structurally incompatible with an extension-backed
/// virtual table, so those are rejected with a clear error.
fn is_virtual_table_ddl(ddl: &str) -> bool {
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.to_uppercase().contains("CREATE VIRTUAL TABLE")
}

/// True if `ddl` declares a `WITHOUT ROWID` table. Such tables have no rowid,
/// so `last_insert_rowid()` cannot identify an inserted row and the
/// `_dirsql_internal_rows.rowid_ref` mapping is meaningless. Whitespace is
/// normalized so `WITHOUT   ROWID` and newline-separated forms are detected.
fn is_without_rowid_ddl(ddl: &str) -> bool {
    let normalized = ddl.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.to_uppercase().contains("WITHOUT ROWID")
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
    fn create_table_from_ddl() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE comments (id TEXT PRIMARY KEY, body TEXT, resolved INTEGER)")
            .unwrap();

        let rows = db.query("SELECT * FROM comments").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn create_table_invalid_ddl_returns_error() {
        let db = Db::new().unwrap();
        let result = db.create_table("NOT VALID SQL");
        assert!(result.is_err());
    }

    #[test]
    fn create_table_runs_ddl_verbatim_no_injected_columns() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
    fn pragma_table_info_matches_user_ddl_exactly() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE posts (title TEXT, draft INTEGER)")
            .unwrap();
        assert_eq!(
            db.get_table_columns("posts").unwrap(),
            vec!["title".to_string(), "draft".to_string()]
        );
    }

    #[test]
    fn insert_and_query_rows() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE docs (title TEXT, draft INTEGER)")
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
        db.create_table("CREATE TABLE events (action TEXT, ts INTEGER)")
            .unwrap();

        for (i, action) in ["created", "resolved", "reopened"].iter().enumerate() {
            let row = HashMap::from([
                ("action".into(), Value::Text(action.to_string())),
                ("ts".into(), Value::Integer(i as i64)),
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
        db.create_table("CREATE TABLE comments (id TEXT, body TEXT)")
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
        db.create_table("CREATE TABLE items (name TEXT, count INTEGER)")
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
    fn parse_table_name_simple() {
        assert_eq!(
            parse_table_name("CREATE TABLE comments (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_if_not_exists() {
        assert_eq!(
            parse_table_name("CREATE TABLE IF NOT EXISTS comments (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_no_space_before_paren() {
        assert_eq!(
            parse_table_name("CREATE TABLE t(id TEXT)"),
            Some("t".to_string())
        );
    }

    #[test]
    fn parse_table_name_invalid() {
        assert_eq!(parse_table_name("NOT A DDL"), None);
    }

    #[test]
    fn parse_table_name_empty_after_create_table() {
        assert_eq!(parse_table_name("CREATE TABLE "), None);
    }

    #[test]
    fn parse_table_name_double_quoted() {
        assert_eq!(
            parse_table_name(r#"CREATE TABLE "comments" (id TEXT)"#),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_double_quoted_no_space_before_paren() {
        assert_eq!(
            parse_table_name(r#"CREATE TABLE "comments"(id TEXT)"#),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_backtick_quoted() {
        assert_eq!(
            parse_table_name("CREATE TABLE `comments` (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_bracket_quoted() {
        assert_eq!(
            parse_table_name("CREATE TABLE [comments] (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_double_quote_escape() {
        // `""` inside a double-quoted identifier is one literal `"`.
        assert_eq!(
            parse_table_name(r#"CREATE TABLE "we""ird" (id TEXT)"#),
            Some("we\"ird".to_string())
        );
    }

    #[test]
    fn parse_table_name_if_not_exists_quoted_mixed_case() {
        assert_eq!(
            parse_table_name(r#"CREATE TABLE if not exists "comments" (id TEXT)"#),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_schema_qualified() {
        assert_eq!(
            parse_table_name("CREATE TABLE main.comments (id TEXT)"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_schema_qualified_quoted() {
        assert_eq!(
            parse_table_name(r#"CREATE TABLE "main"."comments" (id TEXT)"#),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_runs_to_end_of_input() {
        assert_eq!(
            parse_table_name("CREATE TABLE comments"),
            Some("comments".to_string())
        );
    }

    #[test]
    fn parse_table_name_missing_name_is_none() {
        assert_eq!(parse_table_name("CREATE TABLE (id TEXT)"), None);
    }

    #[test]
    fn parse_table_name_ignores_leading_line_comment() {
        assert_eq!(
            parse_table_name("-- create table old\nCREATE TABLE t (x TEXT)"),
            Some("t".to_string())
        );
    }

    #[test]
    fn parse_table_name_ignores_leading_block_comment() {
        assert_eq!(
            parse_table_name("/* CREATE TABLE old */ CREATE TABLE t (x TEXT)"),
            Some("t".to_string())
        );
    }

    #[test]
    fn parse_table_name_unterminated_comment_is_none() {
        // Unterminated `--` and `/* */` comments consume the rest of the input,
        // leaving no statement head.
        assert_eq!(parse_table_name("-- no newline, no statement"), None);
        assert_eq!(parse_table_name("/* unterminated CREATE TABLE t"), None);
    }

    #[test]
    fn parse_table_name_rejects_keyword_prefix() {
        // `CREATE TABLES` must not match the `TABLE` keyword prefix.
        assert_eq!(parse_table_name("CREATE TABLES foo (id TEXT)"), None);
    }

    #[test]
    fn parse_table_name_if_prefixed_name_is_not_if_not_exists() {
        // A table whose name merely starts with `if` is not `IF NOT EXISTS`.
        assert_eq!(
            parse_table_name("CREATE TABLE ifx (id TEXT)"),
            Some("ifx".to_string())
        );
    }

    #[test]
    fn parse_table_name_unicode_in_comment_does_not_panic() {
        // A case-length-changing char (`ﬁ`) in a comment must neither hijack
        // the name nor panic on a byte-index slice.
        assert_eq!(
            parse_table_name("-- ﬁ\nCREATE TABLE t (x TEXT)"),
            Some("t".to_string())
        );
        assert_eq!(
            parse_table_name("/* ﬁﬁﬁ */ CREATE TABLE t (x TEXT)"),
            Some("t".to_string())
        );
    }

    #[test]
    fn get_table_columns_returns_user_columns_only() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (name TEXT, count INTEGER)")
            .unwrap();
        let cols = db.get_table_columns("t").unwrap();
        assert!(cols.contains(&"name".to_string()));
        assert!(cols.contains(&"count".to_string()));
        assert!(!cols.iter().any(|c| c.starts_with("_dirsql_")));
    }

    #[test]
    fn normalize_row_relaxed_drops_extra_keys() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (name TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (name TEXT, color TEXT)")
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
        db.create_table("CREATE TABLE t (name TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (name TEXT, color TEXT)")
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
        db.create_table("CREATE TABLE t (name TEXT, color TEXT)")
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
        db.create_table("CREATE TABLE t (price REAL)").unwrap();
        let row = HashMap::from([("price".into(), Value::Real(9.99))]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT price FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["price"], Value::Real(9.99));
    }

    #[test]
    fn insert_and_query_null_value() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (name TEXT)").unwrap();
        let row = HashMap::from([("name".into(), Value::Null)]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT name FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], Value::Null);
    }

    #[test]
    fn insert_and_query_blob_value() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (data BLOB)").unwrap();
        let row = HashMap::from([("data".into(), Value::Blob(vec![0xFF, 0x00]))]);
        db.insert_row("t", &row, "test.json", 0).unwrap();
        let results = db.query("SELECT data FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["data"], Value::Blob(vec![0xFF, 0x00]));
    }

    #[test]
    fn select_star_returns_only_user_columns() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();
        let results = db.query("SELECT * FROM t").unwrap();
        assert_eq!(results[0].len(), 1);
        assert!(results[0].contains_key("id"));
    }

    #[test]
    fn dirsql_columns_no_longer_exist_on_user_tables() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let rows = db.query("PRAGMA table_info(t)").unwrap();
        assert_eq!(rows.len(), 1, "expected one column row, got {rows:?}");
        assert_eq!(rows[0]["name"], Value::Text("id".into()));
    }

    #[test]
    fn query_authorizer_is_cleared_after_each_query() {
        // `delete_rows_by_file` reads `_dirsql_internal_rows` without routing
        // through query(), so a leaked authorizer would make it fail.
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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

        let err = DbError::DdlParse("bad ddl".to_string());
        assert!(err.to_string().contains("DDL parse error"));
    }

    #[test]
    fn delete_rows_by_file_returns_zero_for_no_matching_rows() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
    fn get_rows_by_file_returns_rows_in_row_index_order() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        db.create_table("CREATE TABLE u (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n TEXT)")
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
    fn create_table_without_parseable_name_fails_at_parse() {
        let db = Db::new().unwrap();
        let err = db
            .create_table("CREATE TABLE (this is not valid)")
            .unwrap_err();
        assert!(matches!(err, DbError::DdlParse(_)), "got: {err}");
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
            .create_table("CREATE TABLE evil;DROP_TABLE--(id TEXT)")
            .unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err:?}");
    }

    #[test]
    fn insert_row_rejects_column_name_with_sql_syntax() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id); DROP TABLE t; --".into(), Value::Text("x".into()))]);
        let err = db.insert_row("t", &row, "f.json", 0).unwrap_err();
        assert!(matches!(err, DbError::InvalidIdentifier(_)), "got: {err:?}");
    }

    #[test]
    fn insert_row_round_trips_reserved_word_column() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (path TEXT, \"order\" INTEGER)")
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
    fn create_table_rejects_virtual_table_with_clear_error() {
        let db = Db::new().unwrap();
        let err = db
            .create_table("CREATE VIRTUAL TABLE vss USING vec0(embedding float[4])")
            .unwrap_err();
        // Must be a clear "not supported" message, NOT the generic `DdlParse`
        // echo (which trivially contains "virtual table" because it echoes
        // the DDL back).
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("virtual table") && msg.contains("not supported"),
            "expected a clear 'virtual table not supported' error, not a generic DDL-parse echo, got: {err}"
        );
    }

    #[test]
    fn is_virtual_table_ddl_detects_variants() {
        assert!(is_virtual_table_ddl("CREATE VIRTUAL TABLE x USING vec0(a)"));
        assert!(is_virtual_table_ddl(
            "create   virtual   table x using fts5(a)"
        ));
        assert!(!is_virtual_table_ddl("CREATE TABLE x (a TEXT)"));
        assert!(!is_virtual_table_ddl(
            "CREATE TABLE IF NOT EXISTS x (a TEXT)"
        ));
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT)")
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
    fn delete_rows_by_file_removes_mapping() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t1 (id TEXT)").unwrap();
        db.create_table("CREATE TABLE t2 (id TEXT)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT UNIQUE)").unwrap();
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
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
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
    fn create_table_allows_without_rowid_and_warns() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT PRIMARY KEY) WITHOUT ROWID")
            .unwrap();
        let rows = db.query("SELECT * FROM t").unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn is_without_rowid_ddl_detects_variants() {
        assert!(is_without_rowid_ddl(
            "CREATE TABLE t (id TEXT PRIMARY KEY) WITHOUT ROWID"
        ));
        assert!(is_without_rowid_ddl(
            "create table t (id text primary key)\n  without   rowid"
        ));
        assert!(!is_without_rowid_ddl("CREATE TABLE t (id TEXT)"));
    }
}
