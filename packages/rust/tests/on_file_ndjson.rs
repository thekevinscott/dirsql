//! The `on-file` output contract: one JSON object per line (NDJSON).
#![cfg(unix)]

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

fn build(root: &TempDir, file: &str) -> Result<DirSQL, dirsql::DirSqlError> {
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (id TEXT, n INTEGER)"
glob = "*.dat"
on-file = "cat"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.dat"), file).unwrap();
    DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
}

#[test]
fn one_object_per_line_becomes_one_row_each() {
    let root = TempDir::new().unwrap();
    let db = build(&root, "{\"id\":\"a\",\"n\":1}\n\n{\"id\":\"b\",\"n\":2}\n").unwrap();
    let rows = db.query("SELECT id, n FROM items ORDER BY id").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["id"], Value::Text("b".into()));
    assert_eq!(rows[1]["n"], Value::Integer(2));
}

#[test]
fn empty_output_is_a_table_with_no_rows() {
    let root = TempDir::new().unwrap();
    let db = build(&root, "").unwrap();
    assert!(db.query("SELECT id FROM items").unwrap().is_empty());
}

#[test]
fn an_array_is_rejected_with_a_message_naming_ndjson() {
    let root = TempDir::new().unwrap();
    let err = build(&root, "[{\"id\":\"a\",\"n\":1}]\n")
        .err()
        .expect("array output fails the build")
        .to_string();
    assert!(err.contains("one JSON object per line"), "{err}");
    assert!(err.contains("array"), "{err}");
}

#[test]
fn a_non_object_line_names_its_line_number() {
    let root = TempDir::new().unwrap();
    let err = build(&root, "{\"id\":\"a\",\"n\":1}\nchatter\n")
        .err()
        .expect("a bad line fails the build")
        .to_string();
    assert!(err.contains("line 2"), "{err}");
}
