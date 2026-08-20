//! Integration tests: dirsql's internal bookkeeping tables are unreachable
//! through the public `query()` surface.
//!
//! `_dirsql_internal_rows`, `_dirsql_files`, and `_dirsql_meta` are engine
//! bookkeeping. A caller reaching `query()` (SDK user, HTTP client) must not be
//! able to read them: a `SELECT` targeting one is rejected at prepare time by a
//! SQLite authorizer installed on the query path. The authorizer applies only
//! to `query()` — the engine still writes these tables while indexing — so this
//! suite also asserts normal user queries and the indexed user rows are intact.

use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::path::Path;
use tempfile::TempDir;

fn user_table() -> Table {
    Table::new("items", "CREATE TABLE items (name TEXT)", "*.txt", |path| {
        let content = std::fs::read_to_string(path).unwrap();
        vec![HashMap::from([(
            "name".into(),
            Value::Text(content.trim().to_string()),
        )])]
    })
}

/// A **persisted** `DirSQL` over a seeded temp dir, so all three internal
/// tables exist on the queried connection: `_dirsql_internal_rows` (created on
/// every `Db`) plus the `_dirsql_files` / `_dirsql_meta` persistence sidecars.
fn persisted_db(root: &Path) -> DirSQL {
    std::fs::write(root.join("a.txt"), "apple").unwrap();
    std::fs::write(root.join("b.txt"), "banana").unwrap();
    DirSQL::builder()
        .root(root)
        .table(user_table())
        .persist(None::<&Path>)
        .build()
        .unwrap()
}

/// Reading `table` through `query()` must be rejected with a "not authorized"
/// error rather than returning rows.
fn assert_rejected(db: &DirSQL, table: &str) {
    match db.query(&format!("SELECT * FROM {table}")) {
        Ok(rows) => panic!(
            "reading internal table `{table}` through query() must be rejected, \
             got {} row(s)",
            rows.len()
        ),
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(
                msg.contains("not authorized"),
                "error for `{table}` should say it is not authorized, got: {e}"
            );
        }
    }
}

#[test]
fn query_rejects_internal_rows_table() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    assert_rejected(&db, "_dirsql_internal_rows");
}

#[test]
fn query_rejects_files_table() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    assert_rejected(&db, "_dirsql_files");
}

#[test]
fn query_rejects_meta_table() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    assert_rejected(&db, "_dirsql_meta");
}

#[test]
fn query_rejects_internal_table_named_in_explicit_projection() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    let result = db.query("SELECT file_path, rowid_ref FROM _dirsql_internal_rows");
    assert!(
        result.is_err(),
        "explicit-column read of an internal table must be rejected, got {:?}",
        result.map(|r| r.len())
    );
}

#[test]
fn query_rejects_internal_table_in_join_with_user_table() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    let result = db.query(
        "SELECT items.name FROM items \
         JOIN _dirsql_internal_rows m ON m.rowid_ref = items.rowid",
    );
    assert!(
        result.is_err(),
        "a join touching an internal table must be rejected, got {:?}",
        result.map(|r| r.len())
    );
}

#[test]
fn query_allows_normal_user_table() {
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert_eq!(rows[1]["name"], Value::Text("banana".into()));
}

#[test]
fn internal_write_paths_unaffected_user_rows_present() {
    // The authorizer never fires on the engine's own internal writes during
    // indexing, so the indexed user rows are all present.
    let root = TempDir::new().unwrap();
    let db = persisted_db(root.path());
    let rows = db.query("SELECT COUNT(*) AS n FROM items").unwrap();
    assert_eq!(rows[0]["n"], Value::Integer(2));
}
