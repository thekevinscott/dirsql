//! A table may be anchored away from the index root: its glob matches paths
//! relative to its own anchor, and the index root keeps governing the rest.

use dirsql::{DirSQL, Row, Table, Value};
use std::fs;
use tempfile::TempDir;

fn path_row(path: &str) -> Vec<Row> {
    vec![Row::from([("path".to_string(), Value::Text(path.into()))])]
}

#[test]
fn an_anchored_table_scans_its_anchor_not_the_index_root() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("decoy.txt"), "x").unwrap();
    let anchor = TempDir::new().unwrap();
    fs::write(anchor.path().join("present.txt"), "x").unwrap();

    let table = Table::new("items", "CREATE TABLE items (path TEXT)", "*.txt", |path| {
        path_row(path)
    })
    .anchored(anchor.path());

    let db = DirSQL::builder()
        .root(root.path())
        .table(table)
        .build()
        .unwrap();

    let rows = db.query("SELECT path FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    let Value::Text(path) = &rows[0]["path"] else {
        panic!("path is text");
    };
    assert!(path.ends_with("present.txt"), "got {path}");
}

#[test]
fn an_unanchored_table_still_scans_the_index_root() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.txt"), "x").unwrap();
    let anchor = TempDir::new().unwrap();
    fs::write(anchor.path().join("b.txt"), "x").unwrap();

    let at_root = Table::new(
        "at_root",
        "CREATE TABLE at_root (path TEXT)",
        "*.txt",
        path_row,
    );
    let elsewhere = Table::new(
        "elsewhere",
        "CREATE TABLE elsewhere (path TEXT)",
        "*.txt",
        path_row,
    )
    .anchored(anchor.path());

    let db = DirSQL::builder()
        .root(root.path())
        .tables(vec![at_root, elsewhere])
        .build()
        .unwrap();

    let a = db.query("SELECT path FROM at_root").unwrap();
    let b = db.query("SELECT path FROM elsewhere").unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_ne!(a[0]["path"], b[0]["path"]);
}
