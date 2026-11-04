//! Integration tests: an `on-file` key fills the declared column it names
//! under SQL's ASCII-case-insensitive identifier rule, so a key whose case
//! differs from the DDL's still lands in that column.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

fn title(strict: bool) -> Value {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("t.ndjson"), "{\"title\":\"hello\"}\n").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            r#"
[[table]]
name = "posts"
ddl = "CREATE TABLE posts (TITLE TEXT)"
glob = "t.ndjson"
strict = {strict}
on-file = "cat"
"#
        ),
    )
    .unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT title FROM posts").unwrap();
    assert_eq!(rows.len(), 1);
    rows[0]["TITLE"].clone()
}

#[test]
fn a_key_differing_from_its_column_only_by_case_fills_that_column() {
    assert_eq!(title(false), Value::Text("hello".into()));
}

#[test]
fn a_strict_table_accepts_a_key_differing_from_its_column_only_by_case() {
    assert_eq!(title(true), Value::Text("hello".into()));
}
