use rusqlite::Connection;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema mismatch: {0}")]
    SchemaMismatch(String),

    #[error("DDL parse error: {0}")]
    DdlParse(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    /// Create a table from a user-provided DDL statement.
    /// Automatically injects internal tracking columns (_dirsql_file_path, _dirsql_row_index).
    pub fn create_table(&self, ddl: &str) -> Result<()> {
        let augmented = inject_tracking_columns(ddl)?;
        self.conn.execute(&augmented, [])?;
        Ok(())
    }

    /// Return the user-defined column names for `table` (excludes `_dirsql_*` tracking columns).
    pub fn get_table_columns(&self, table: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({})", table))?;
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .filter(|name| !name.starts_with("_dirsql_"))
            .collect();
        Ok(columns)
    }

    /// Normalize a row to match the table schema.
    ///
    /// In relaxed mode (strict=false): extra keys are dropped, missing keys become NULL.
    /// In strict mode (strict=true): extra or missing keys produce a SchemaMismatch error.
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
    pub fn insert_row(
        &self,
        table: &str,
        row: &HashMap<String, Value>,
        file_path: &str,
        row_index: usize,
    ) -> Result<()> {
        let mut columns: Vec<String> = row.keys().cloned().collect();
        columns.push("_dirsql_file_path".to_string());
        columns.push("_dirsql_row_index".to_string());

        let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("?{}", i)).collect();

        let sql = format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table,
            columns.join(", "),
            placeholders.join(", "),
        );

        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = row
            .values()
            .map(|v| Box::new(v.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params.push(Box::new(file_path.to_string()));
        params.push(Box::new(row_index as i64));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        self.conn.execute(&sql, param_refs.as_slice())?;
        Ok(())
    }

    /// Delete all rows that were produced by a given file path.
    pub fn delete_rows_by_file(&self, table: &str, file_path: &str) -> Result<usize> {
        let sql = format!("DELETE FROM {} WHERE _dirsql_file_path = ?1", table);
        let count = self.conn.execute(&sql, [file_path])?;
        Ok(count)
    }

    /// Query the database, returning rows as a list of column-name -> value maps.
    ///
    /// Internal tracking columns (`_dirsql_*`) are excluded from `SELECT *`
    /// results so they don't leak. But if the user names one explicitly in the
    /// projection (e.g. `SELECT _dirsql_file_path FROM t`), it's returned —
    /// users opt into the tracking surface by typing the column name.
    pub fn query(&self, sql: &str) -> Result<Vec<HashMap<String, Value>>> {
        let mut stmt = self.conn.prepare(sql)?;
        let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

        let rows = stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for (i, name) in column_names.iter().enumerate() {
                if name.starts_with("_dirsql_") && !sql.contains(name) {
                    continue;
                }
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

/// Inject _dirsql_file_path and _dirsql_row_index columns into a CREATE TABLE DDL statement.
fn inject_tracking_columns(ddl: &str) -> Result<String> {
    // Find the last closing paren in the DDL and insert our columns before it
    let close_paren = ddl
        .rfind(')')
        .ok_or_else(|| DbError::DdlParse("DDL must contain a closing parenthesis".to_string()))?;

    let before = &ddl[..close_paren];
    let after = &ddl[close_paren..];

    Ok(format!(
        "{}, _dirsql_file_path TEXT NOT NULL, _dirsql_row_index INTEGER NOT NULL{}",
        before, after
    ))
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

/// Extract the table name from a CREATE TABLE DDL statement.
/// Handles: CREATE TABLE name (...), CREATE TABLE IF NOT EXISTS name (...)
pub fn parse_table_name(ddl: &str) -> Option<String> {
    let upper = ddl.to_uppercase();
    let idx = upper.find("CREATE TABLE")?;
    let rest = &ddl[idx + "CREATE TABLE".len()..].trim_start();

    // Skip optional "IF NOT EXISTS"
    let rest = if rest.to_uppercase().starts_with("IF NOT EXISTS") {
        rest["IF NOT EXISTS".len()..].trim_start()
    } else {
        rest
    };

    // Table name is everything up to the first whitespace or '('
    let name: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '(')
        .collect();

    if name.is_empty() { None } else { Some(name) }
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
    use rusqlite::types::ToSql;

    #[test]
    fn create_table_from_ddl() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE comments (id TEXT PRIMARY KEY, body TEXT, resolved INTEGER)")
            .unwrap();

        // Table should exist -- querying it should return empty results
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
    fn create_table_injects_tracking_columns() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();

        // The tracking columns should exist even though the user didn't declare them
        db.insert_row(
            "t",
            &HashMap::from([("id".into(), Value::Text("1".into()))]),
            "test.json",
            0,
        )
        .unwrap();

        // SELECT * should NOT return tracking columns
        let rows = db.query("SELECT * FROM t").unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains_key("id"));
        assert!(!rows[0].contains_key("_dirsql_file_path"));
        assert!(!rows[0].contains_key("_dirsql_row_index"));
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

        // Insert rows from two different files
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

        // Delete rows from file "a.jsonl"
        let deleted = db.delete_rows_by_file("comments", "a.jsonl").unwrap();
        assert_eq!(deleted, 2);

        // Only file b's row remains
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
    fn inject_tracking_columns_modifies_ddl() {
        let result = inject_tracking_columns("CREATE TABLE t (id TEXT)").unwrap();
        assert!(result.contains("_dirsql_file_path TEXT NOT NULL"));
        assert!(result.contains("_dirsql_row_index INTEGER NOT NULL"));
    }

    #[test]
    fn inject_tracking_columns_rejects_missing_paren() {
        let result = inject_tracking_columns("NOT A CREATE TABLE");
        assert!(result.is_err());
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

    // --- Value::to_sql coverage for all variants ---

    #[test]
    fn value_to_sql_null() {
        let v = Value::Null;
        let result = v.to_sql().unwrap();
        assert!(matches!(
            result,
            rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Null)
        ));
    }

    #[test]
    fn value_to_sql_integer() {
        let v = Value::Integer(42);
        let result = v.to_sql().unwrap();
        assert!(matches!(
            result,
            rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Integer(42))
        ));
    }

    #[test]
    fn value_to_sql_real() {
        let v = Value::Real(3.14);
        let result = v.to_sql().unwrap();
        match result {
            rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Real(f)) => {
                assert!((f - 3.14).abs() < f64::EPSILON);
            }
            _ => panic!("expected Real"),
        }
    }

    #[test]
    fn value_to_sql_text() {
        let v = Value::Text("hello".into());
        let result = v.to_sql().unwrap();
        assert!(matches!(
            result,
            rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Text(ref s)) if s == "hello"
        ));
    }

    #[test]
    fn value_to_sql_blob() {
        let v = Value::Blob(vec![1, 2, 3]);
        let result = v.to_sql().unwrap();
        assert!(matches!(
            result,
            rusqlite::types::ToSqlOutput::Owned(rusqlite::types::Value::Blob(ref b)) if b == &[1, 2, 3]
        ));
    }

    // --- Value::from coverage for all variants ---

    #[test]
    fn value_from_sqlite_null() {
        let v = Value::from(rusqlite::types::Value::Null);
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn value_from_sqlite_integer() {
        let v = Value::from(rusqlite::types::Value::Integer(99));
        assert_eq!(v, Value::Integer(99));
    }

    #[test]
    fn value_from_sqlite_real() {
        let v = Value::from(rusqlite::types::Value::Real(2.718));
        assert_eq!(v, Value::Real(2.718));
    }

    #[test]
    fn value_from_sqlite_text() {
        let v = Value::from(rusqlite::types::Value::Text("world".into()));
        assert_eq!(v, Value::Text("world".into()));
    }

    #[test]
    fn value_from_sqlite_blob() {
        let v = Value::from(rusqlite::types::Value::Blob(vec![10, 20]));
        assert_eq!(v, Value::Blob(vec![10, 20]));
    }

    // --- Insert and query with real/blob values ---

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

    // --- Query that returns _dirsql_ columns via explicit SELECT ---

    #[test]
    fn query_filters_dirsql_columns_from_star() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();
        // SELECT * should not include _dirsql_ columns
        let results = db.query("SELECT * FROM t").unwrap();
        assert_eq!(results[0].len(), 1);
        assert!(results[0].contains_key("id"));
    }

    #[test]
    fn query_honors_explicit_dirsql_file_path() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();

        let results = db.query("SELECT _dirsql_file_path FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].get("_dirsql_file_path"),
            Some(&Value::Text("file.json".into())),
        );
    }

    #[test]
    fn query_honors_explicit_dirsql_alongside_user_columns() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE posts (title TEXT)").unwrap();
        let row = HashMap::from([("title".into(), Value::Text("Hello".into()))]);
        db.insert_row("posts", &row, "posts/hello.json", 0).unwrap();

        let results = db
            .query("SELECT title, _dirsql_file_path FROM posts")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], Value::Text("Hello".into()));
        assert_eq!(
            results[0]["_dirsql_file_path"],
            Value::Text("posts/hello.json".into()),
        );
    }

    #[test]
    fn query_honors_explicit_dirsql_row_index() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("a".into()))]);
        db.insert_row("t", &row, "f.jsonl", 7).unwrap();

        let results = db.query("SELECT _dirsql_row_index FROM t").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["_dirsql_row_index"], Value::Integer(7));
    }

    #[test]
    fn query_keeps_dirsql_when_filter_references_it_with_star_projection() {
        // Naming `_dirsql_file_path` anywhere in the SQL is treated as
        // "the user is aware of this tracking column", so `SELECT *` with
        // a `_dirsql_*` reference in WHERE returns it.
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "file.json", 0).unwrap();

        let results = db
            .query("SELECT * FROM t WHERE _dirsql_file_path = 'file.json'")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].contains_key("id"));
        assert!(results[0].contains_key("_dirsql_file_path"));
    }

    // --- Error path: query with invalid SQL ---

    #[test]
    fn query_invalid_sql_returns_error() {
        let db = Db::new().unwrap();
        let result = db.query("SELECT FROM nonexistent");
        assert!(result.is_err());
    }

    // --- Error path: insert into nonexistent table ---

    #[test]
    fn insert_into_nonexistent_table_returns_error() {
        let db = Db::new().unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        let result = db.insert_row("nonexistent", &row, "f.json", 0);
        assert!(result.is_err());
    }

    // --- Error path: delete from nonexistent table ---

    #[test]
    fn delete_from_nonexistent_table_returns_error() {
        let db = Db::new().unwrap();
        let result = db.delete_rows_by_file("nonexistent", "f.json");
        assert!(result.is_err());
    }

    // --- Error path: get_table_columns on nonexistent table returns empty ---

    #[test]
    fn get_table_columns_nonexistent_table_returns_empty() {
        let db = Db::new().unwrap();
        let cols = db.get_table_columns("nonexistent").unwrap();
        assert!(cols.is_empty());
    }

    // --- DbError Display ---

    #[test]
    fn db_error_display_messages() {
        let err = DbError::SchemaMismatch("test error".to_string());
        assert!(err.to_string().contains("Schema mismatch"));

        let err = DbError::DdlParse("bad ddl".to_string());
        assert!(err.to_string().contains("DDL parse error"));
    }

    // --- delete_rows_by_file returns zero when no rows match ---

    #[test]
    fn delete_rows_by_file_returns_zero_for_no_matching_rows() {
        let db = Db::new().unwrap();
        db.create_table("CREATE TABLE t (id TEXT)").unwrap();
        let row = HashMap::from([("id".into(), Value::Text("1".into()))]);
        db.insert_row("t", &row, "a.json", 0).unwrap();
        let deleted = db.delete_rows_by_file("t", "nonexistent.json").unwrap();
        assert_eq!(deleted, 0);
    }
}
