//! Integration tests: a hook's key fills the declared column it names under
//! SQL's ASCII-case-insensitive identifier rule, so a key whose case differs
//! from the DDL's still lands in that column. Covers both the config
//! `on-file` command and an SDK `Table` closure.

use std::collections::HashMap;
use std::fs;

use dirsql::{DirSQL, Table, Value};
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

fn sdk_title(strict: bool) -> Value {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("t.txt"), "hello").unwrap();
    let hook = |_: &str| {
        vec![HashMap::from([(
            "title".into(),
            Value::Text("hello".into()),
        )])]
    };
    let table = if strict {
        Table::strict("posts", "CREATE TABLE posts (TITLE TEXT)", "t.txt", hook)
    } else {
        Table::new("posts", "CREATE TABLE posts (TITLE TEXT)", "t.txt", hook)
    };
    let db = DirSQL::new(root.path(), vec![table]).unwrap();
    assert!(db.scan_failures().is_empty(), "{:?}", db.scan_failures());
    let rows = db.query("SELECT title FROM posts").unwrap();
    assert_eq!(rows.len(), 1);
    rows[0]["TITLE"].clone()
}

#[test]
fn an_sdk_hook_key_differing_from_its_column_only_by_case_fills_that_column() {
    assert_eq!(sdk_title(false), Value::Text("hello".into()));
}

#[test]
fn a_strict_sdk_table_accepts_a_key_differing_from_its_column_only_by_case() {
    assert_eq!(sdk_title(true), Value::Text("hello".into()));
}
