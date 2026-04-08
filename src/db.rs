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
    /// Internal tracking columns (_dirsql_*) are excluded from results.
    pub fn query(&self, sql: &str) -> Result<Vec<HashMap<String, Value>>> {
        let mut stmt = self.conn.prepare(sql)?;
        let column_names: Vec<String> = stmt
            .column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let rows = stmt.query_map([], |row| {
            let mut map = HashMap::new();
            for (i, name) in column_names.iter().enumerate() {
                if name.starts_with("_dirsql_") {
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
    let close_paren = ddl.rfind(')').ok_or_else(|| {
        DbError::DdlParse("DDL must contain a closing parenthesis".to_string())
    })?;

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
        db.create_table(
            "CREATE TABLE comments (id TEXT PRIMARY KEY, body TEXT, resolved INTEGER)",
        )
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
        db.insert_row("t", &HashMap::from([("id".into(), Value::Text("1".into()))]), "test.json", 0)
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
        db.create_table("CREATE TABLE docs (title TEXT, draft INTEGER)").unwrap();

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
        db.create_table("CREATE TABLE events (action TEXT, ts INTEGER)").unwrap();

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
        db.create_table("CREATE TABLE comments (id TEXT, body TEXT)").unwrap();

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
        db.create_table("CREATE TABLE items (name TEXT, count INTEGER)").unwrap();

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
}
