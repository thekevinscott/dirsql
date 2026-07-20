//! Integration tests for the retirement of the implicit no-config `files`
//! table. With no config and no programmatic tables, dirsql defines no named
//! tables at all; path-tables serve filesystem queries. A `files` query in that
//! state fails, and only in that state does it carry the path-table hint.
//! Real filesystem, real SQLite, SDK public API.

use std::collections::HashMap;
use std::fs;

use dirsql::{DirSQL, Table, Value};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("readme.md"), "hello").unwrap();
    root
}

/// A user-declared table that is deliberately *not* named `files`.
fn posts_table() -> Table {
    Table::new("CREATE TABLE posts (path TEXT)", "**/*.md", |path| {
        vec![HashMap::from([(
            "path".into(),
            Value::Text(path.to_string()),
        )])]
    })
}

#[test]
fn a_no_config_builder_defines_no_named_tables() {
    let root = fixture();

    let db = DirSQL::builder().root(root.path()).build().unwrap();
    let err = db.query("SELECT * FROM files").unwrap_err().to_string();

    assert!(
        err.contains("no such table: files"),
        "no-config builds must define no `files` table, got: {err}"
    );
}

#[test]
fn a_no_config_files_query_carries_the_path_table_hint() {
    let root = fixture();

    let db = DirSQL::builder().root(root.path()).build().unwrap();
    let err = db.query("SELECT * FROM files").unwrap_err().to_string();

    assert!(
        err.contains("did you mean FROM './'?"),
        "the no-config `files` miss must point at the path-table form, got: {err}"
    );
}

#[test]
fn a_no_config_path_table_query_still_works() {
    let root = fixture();

    let db = DirSQL::builder().root(root.path()).build().unwrap();
    let rows = db.query("SELECT basename FROM './'").unwrap();

    let names: Vec<Value> = rows.iter().map(|r| r["basename"].clone()).collect();
    assert!(
        names.contains(&Value::Text("readme.md".into())),
        "`FROM './'` is the replacement for the retired default table, got {names:?}"
    );
}

#[test]
fn a_user_table_set_that_omits_files_gets_the_plain_error() {
    let root = fixture();

    let db = DirSQL::new(root.path(), vec![posts_table()]).unwrap();
    let err = db.query("SELECT * FROM files").unwrap_err().to_string();

    assert!(
        err.contains("no such table: files"),
        "got: {err}"
    );
    assert!(
        !err.contains("did you mean"),
        "a user who defined tables and forgot `files` gets the plain error, got: {err}"
    );
}

#[test]
fn a_configured_table_set_that_omits_files_gets_the_plain_error() {
    let root = fixture();
    let config = root.path().join("dirsql.toml");
    fs::write(
        &config,
        "[[table]]\nddl = \"CREATE TABLE posts (path TEXT)\"\nglob = \"**/*.md\"\n",
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(&config)
        .build()
        .unwrap();
    let err = db.query("SELECT * FROM files").unwrap_err().to_string();

    assert!(
        !err.contains("did you mean"),
        "a config that forgot `files` gets the plain error, got: {err}"
    );
}

#[test]
fn a_no_config_miss_on_another_name_gets_the_plain_error() {
    let root = fixture();

    let db = DirSQL::builder().root(root.path()).build().unwrap();
    let err = db.query("SELECT * FROM fyles").unwrap_err().to_string();

    assert!(
        err.contains("no such table: fyles"),
        "got: {err}"
    );
    assert!(
        !err.contains("did you mean"),
        "the hint is scoped to the exact name `files`, got: {err}"
    );
}
