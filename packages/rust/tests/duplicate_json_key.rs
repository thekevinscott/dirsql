//! Integration tests: a key repeated within one hook row keeps its last
//! value, whether the row reaches a path-table through the SDK's
//! `path_table_parser` or a declared table through a config `on-file`.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("d.ndjson"),
        "{\"k\":\"first\",\"k\":\"second\"}\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"dup\"\nddl = \"CREATE TABLE dup (k TEXT)\"\nglob = \"d.ndjson\"\non-file = \"cat\"\n",
    )
    .unwrap();
    root
}

fn single_k(db: &DirSQL, sql: &str) -> Value {
    let rows = db.query(sql).unwrap();
    assert_eq!(rows.len(), 1);
    rows[0]["k"].clone()
}

#[test]
fn a_path_table_parser_keeps_the_last_value_of_a_repeated_key() {
    let root = fixture();
    let db = DirSQL::builder()
        .root(root.path())
        .path_table_parser("cat")
        .build()
        .unwrap();
    assert_eq!(
        single_k(&db, "SELECT k FROM './d.ndjson'"),
        Value::Text("second".into())
    );
}

#[test]
fn a_config_table_keeps_the_last_value_of_a_repeated_key() {
    let root = fixture();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    assert_eq!(
        single_k(&db, "SELECT k FROM dup"),
        Value::Text("second".into())
    );
}
