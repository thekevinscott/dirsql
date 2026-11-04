//! A per-file hook that fails, or returns a row the table rejects, fails the
//! build with an error naming the file, the table and the original error.

use std::fs;

use dirsql::{DirSQL, DirSqlError, Row, Table, Value};
use tempfile::TempDir;

fn tree(files: &[&str]) -> TempDir {
    let root = TempDir::new().unwrap();
    for name in files {
        fs::write(root.path().join(name), "x").unwrap();
    }
    root
}

fn row(pairs: &[(&str, Value)]) -> Row {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn build_err(root: &TempDir, tables: Vec<Table>) -> DirSqlError {
    DirSQL::new(root.path(), tables)
        .err()
        .expect("the build must fail")
}

#[test]
fn a_not_null_violation_fails_the_build() {
    let root = tree(&["a.txt"]);
    let err = build_err(
        &root,
        vec![Table::new(
            "notes",
            "CREATE TABLE notes (n INTEGER NOT NULL)",
            "*.txt",
            |_| vec![row(&[("n", Value::Null)])],
        )],
    );
    assert!(
        matches!(&err, DirSqlError::OnFile { table, path, message }
            if table == "notes" && path == "a.txt" && message.contains("NOT NULL")),
        "{err:?}"
    );
}

#[test]
fn a_unique_violation_fails_the_build() {
    let root = tree(&["a.txt", "b.txt"]);
    let err = build_err(
        &root,
        vec![Table::new(
            "notes",
            "CREATE TABLE notes (id TEXT UNIQUE)",
            "*.txt",
            |_| vec![row(&[("id", Value::Text("same".into()))])],
        )],
    );
    assert!(
        matches!(&err, DirSqlError::OnFile { table, message, .. }
            if table == "notes" && message.contains("UNIQUE")),
        "{err:?}"
    );
}

#[test]
fn a_parent_hook_failure_is_reported_ahead_of_the_orphaned_child() {
    let root = tree(&["b.txt"]);
    let err = build_err(
        &root,
        vec![
            Table::try_new(
                "parent",
                "CREATE TABLE parent (id TEXT PRIMARY KEY)",
                "*.txt",
                |_| Err("rejected-xyzzy".into()),
            ),
            Table::new(
                "child",
                "CREATE TABLE child (parent TEXT REFERENCES parent(id))",
                "*.txt",
                |_| vec![row(&[("parent", Value::Text("b".into()))])],
            ),
        ],
    );
    assert_eq!(
        err.to_string(),
        "table `parent`, file `b.txt`: rejected-xyzzy"
    );
}
