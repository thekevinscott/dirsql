//! Integration tests: `query()` must reject statements that change the shared
//! connection's session state.
//!
//! SQLite classifies `BEGIN`, `SAVEPOINT` and most `PRAGMA` setters as
//! read-only, so the read-only gate lets them through. Run on the one
//! connection every query shares, they outlive the statement: an open
//! transaction hides the watcher's next write, and a setter changes what every
//! later statement returns.

use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn items_db(root: &Path) -> DirSQL {
    fs::write(root.join("a.txt"), "apple").unwrap();
    DirSQL::new(
        root,
        vec![Table::new(
            "items",
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap()
}

fn assert_rejected(db: &DirSQL, sql: &str) {
    let err = db
        .query(sql)
        .expect_err(&format!("{sql:?} must be rejected"));
    assert!(
        err.to_string().contains("not authorized"),
        "{sql:?} should be rejected as not authorized, got: {err}"
    );
}

fn like_count(db: &DirSQL) -> Value {
    let rows = db
        .query("SELECT count(*) AS n FROM items WHERE name LIKE 'APPLE'")
        .unwrap();
    rows[0]["n"].clone()
}

#[test]
fn query_rejects_transaction_control() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    for sql in [
        "BEGIN",
        "BEGIN IMMEDIATE",
        "COMMIT",
        "END",
        "ROLLBACK",
        "SAVEPOINT a",
        "RELEASE a",
        "ROLLBACK TO a",
    ] {
        assert_rejected(&db, sql);
    }
}

#[test]
fn query_rejects_pragma_setters() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    for sql in [
        "PRAGMA case_sensitive_like=1",
        "pragma CASE_SENSITIVE_LIKE = 1",
        "PRAGMA full_column_names=1",
        "PRAGMA short_column_names=0",
        "PRAGMA hard_heap_limit=100000",
        "PRAGMA query_only=1",
        "PRAGMA reverse_unordered_selects=1",
        "PRAGMA foreign_keys=1",
        "PRAGMA locking_mode=EXCLUSIVE",
        "PRAGMA writable_schema=1",
        "PRAGMA cache_size=10",
        "PRAGMA busy_timeout=1",
        "PRAGMA main.cache_size(10)",
    ] {
        assert_rejected(&db, sql);
    }
}

#[test]
fn a_rejected_setter_leaves_later_statements_unchanged() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    assert_eq!(like_count(&db), Value::Integer(1));
    let _ = db.query("PRAGMA case_sensitive_like=1");
    assert_eq!(like_count(&db), Value::Integer(1));
}

#[test]
fn query_still_allows_reading_pragmas() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    let rows = db.query("PRAGMA table_info(items)").unwrap();
    assert_eq!(rows[0]["name"], Value::Text("name".into()));
    let rows = db.query("PRAGMA cache_size").unwrap();
    assert_eq!(rows.len(), 1);
    let rows = db.query("PRAGMA integrity_check(1)").unwrap();
    assert_eq!(rows[0]["integrity_check"], Value::Text("ok".into()));
    let rows = db
        .query("SELECT name FROM pragma_table_info('items')")
        .unwrap();
    assert_eq!(rows[0]["name"], Value::Text("name".into()));
}
