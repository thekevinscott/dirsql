//! Integration tests for the read-only enforcement on `DirSQL::query`.
//!
//! `query()` is the user-facing surface for running SQL over the index. It
//! must only accept read-only statements so that a caller who reaches the
//! method (SDK user, HTTP client, etc.) cannot mutate the in-memory index
//! via `DELETE`, `DROP`, `ATTACH`, `PRAGMA writable_schema`, etc.

use dirsql::{DirSQL, DirSqlError, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn items_db(root: &Path) -> DirSQL {
    fs::write(root.join("a.txt"), "apple").unwrap();
    fs::write(root.join("b.txt"), "banana").unwrap();
    DirSQL::new(
        root,
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap()
}

#[test]
fn it_rejects_delete_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("DELETE FROM items").unwrap_err();
    assert!(
        matches!(err, DirSqlError::WriteForbidden { .. }),
        "expected WriteForbidden, got {err:?}"
    );

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_rejects_drop_table_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("DROP TABLE items").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_rejects_insert_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db
        .query("INSERT INTO items (name) VALUES ('evil')")
        .unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_rejects_update_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("UPDATE items SET name = 'zzz'").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));

    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
}

#[test]
fn it_rejects_attach_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("ATTACH DATABASE ':memory:' AS evil").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));
}

#[test]
fn it_rejects_pragma_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("PRAGMA writable_schema = 1").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));
}

#[test]
fn it_rejects_create_table_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("CREATE TABLE evil (id TEXT)").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));
}

#[test]
fn it_rejects_replace_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db
        .query("REPLACE INTO items (name) VALUES ('x')")
        .unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));
}

#[test]
fn it_rejects_vacuum_via_query() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("VACUUM").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));
}

#[test]
fn it_allows_select_with_leading_whitespace() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let rows = db.query("   \n\t SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_allows_with_cte_queries() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let rows = db
        .query("WITH x AS (SELECT name FROM items) SELECT * FROM x")
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_allows_mixed_case_keywords() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let rows = db.query("select name from items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_rejects_write_after_leading_line_comment() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("-- innocent\nDELETE FROM items").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_rejects_write_after_leading_block_comment() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("/* innocent */ DROP TABLE items").unwrap_err();
    assert!(matches!(err, DirSqlError::WriteForbidden { .. }));

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_allows_select_after_leading_comment() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let rows = db.query("-- heading\nSELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn write_forbidden_error_mentions_rejected_keyword() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("DELETE FROM items").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.to_uppercase().contains("DELETE"),
        "error message should mention DELETE, got: {msg}"
    );
}
