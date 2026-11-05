//! A watch refresh compares and reports rows as the table stores them, after
//! column affinity, not as the hook emitted them.

use dirsql::watcher::FileEvent;
use dirsql::{DirSQL, RowEvent, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const ROW: &str = "{\"k\":\"a\",\"x\":1,\"s\":5,\"i\":\"7\",\"z\":-0.0}\n";

fn open(root: &Path, ddl: &str) -> DirSQL {
    fs::create_dir_all(root.join("d")).unwrap();
    fs::write(root.join("d/a.jsonl"), ROW).unwrap();
    let cfg = root.join(".dirsql.toml");
    fs::write(
        &cfg,
        format!(
            "[[table]]\nname = \"t\"\nddl = \"{ddl}\"\nglob = \"d/*.jsonl\"\non-file = \"cat\"\n"
        ),
    )
    .unwrap();
    DirSQL::builder().root(root).config(&cfg).build().unwrap()
}

fn rewrite_unchanged(ddl: &str) -> Vec<RowEvent> {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let db = open(&root, ddl);
    fs::write(root.join("d/a.jsonl"), ROW).unwrap();
    db.apply_file_events(vec![FileEvent::Modified(root.join("d/a.jsonl"))])
}

#[test]
fn rewriting_a_keyed_file_whose_stored_rows_are_unchanged_emits_nothing() {
    let events =
        rewrite_unchanged("CREATE TABLE t (k TEXT PRIMARY KEY, x REAL, s TEXT, i INTEGER, z REAL)");
    assert!(events.is_empty(), "got: {events:?}");
}

#[test]
fn rewriting_an_unkeyed_file_whose_stored_rows_are_unchanged_emits_nothing() {
    let events = rewrite_unchanged("CREATE TABLE t (k TEXT, x REAL, s TEXT, i INTEGER, z REAL)");
    assert!(events.is_empty(), "got: {events:?}");
}

#[test]
fn an_update_event_reports_the_row_as_stored() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let db = open(
        &root,
        "CREATE TABLE t (k TEXT PRIMARY KEY, x REAL, s TEXT, i INTEGER, z REAL)",
    );
    fs::write(
        root.join("d/a.jsonl"),
        "{\"k\":\"a\",\"x\":2,\"s\":6,\"i\":\"8\",\"z\":-0.0}\n",
    )
    .unwrap();
    let events = db.apply_file_events(vec![FileEvent::Modified(root.join("d/a.jsonl"))]);
    let stored = &db.query("SELECT * FROM t").unwrap()[0];
    match events.as_slice() {
        [RowEvent::Update { new_row, .. }] => assert_same(new_row, stored),
        other => panic!("expected one update, got: {other:?}"),
    }
}

#[test]
fn an_insert_event_reports_the_row_as_stored() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let db = open(
        &root,
        "CREATE TABLE t (k TEXT PRIMARY KEY, x REAL, s TEXT, i INTEGER, z REAL)",
    );
    fs::write(
        root.join("d/a.jsonl"),
        format!(
            "{ROW}{}",
            "{\"k\":\"b\",\"x\":2,\"s\":6,\"i\":\"8\",\"z\":-0.0}\n"
        ),
    )
    .unwrap();
    let events = db.apply_file_events(vec![FileEvent::Modified(root.join("d/a.jsonl"))]);
    let stored = &db.query("SELECT * FROM t WHERE k = 'b'").unwrap()[0];
    match events.as_slice() {
        [RowEvent::Insert { row, .. }] => assert_same(row, stored),
        other => panic!("expected one insert, got: {other:?}"),
    }
}

/// Compared by `Debug`, which tells `-0.0` from `0.0` where `==` does not.
fn assert_same(event: &HashMap<String, Value>, stored: &HashMap<String, Value>) {
    assert_eq!(
        format!("{:?}", sorted(event)),
        format!("{:?}", sorted(stored))
    );
}

fn sorted(row: &HashMap<String, Value>) -> Vec<(&String, &Value)> {
    let mut cells: Vec<_> = row.iter().collect();
    cells.sort_by_key(|(name, _)| *name);
    cells
}
