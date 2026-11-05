//! Integration tests for path-tables whose spellings differ only in letter
//! case. SQLite folds identifier case, but a path names exactly what it spells,
//! so `'./*.md'` and `'./*.MD'` are two tables. Real filesystem, real SQLite,
//! SDK public API.

use std::fs;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// `a.md` and `b.MD`: names that coexist on a case-insensitive filesystem too,
/// told apart only by globs that differ in case.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "lower").unwrap();
    fs::write(root.path().join("b.MD"), "upper").unwrap();
    root
}

fn open(root: &TempDir) -> DirSQL {
    DirSQL::new(root.path(), vec![]).unwrap()
}

fn text(row: &Row, column: &str) -> String {
    match row.get(column) {
        Some(Value::Text(s)) => s.clone(),
        other => panic!("{column} was not text: {other:?}"),
    }
}

fn column(rows: &[Row], name: &str) -> Vec<String> {
    rows.iter().map(|r| text(r, name)).collect()
}

#[test]
fn spellings_differing_in_case_in_one_statement_are_two_tables() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query("SELECT l.path AS lower, u.path AS upper FROM './*.md' l, './*.MD' u")
        .unwrap();

    assert_eq!(column(&rows, "lower"), vec!["a.md"]);
    assert_eq!(column(&rows, "upper"), vec!["b.MD"]);
}

#[test]
fn a_later_statement_does_not_reuse_a_case_variant_table() {
    let root = fixture();
    let db = open(&root);

    let first = db.query("SELECT path FROM './*.md'").unwrap();
    let second = db.query("SELECT path FROM './*.MD'").unwrap();
    let third = db.query("SELECT path FROM './*.md'").unwrap();

    assert_eq!(column(&first, "path"), vec!["a.md"]);
    assert_eq!(column(&second, "path"), vec!["b.MD"]);
    assert_eq!(column(&third, "path"), vec!["a.md"]);
}

#[test]
fn a_literal_spelled_like_a_case_variant_keeps_its_value() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query(
            "SELECT u.path AS path, './*.md' AS lit FROM './*.MD' u \
             WHERE EXISTS (SELECT 1 FROM './*.md')",
        )
        .unwrap();

    assert_eq!(column(&rows, "path"), vec!["b.MD"]);
    assert_eq!(column(&rows, "lit"), vec!["./*.md"]);
}

#[test]
fn a_case_variant_qualifies_its_own_columns() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query(r#"SELECT "./*.md".path AS lower, "./*.MD".path AS upper FROM './*.md', './*.MD'"#)
        .unwrap();

    assert_eq!(column(&rows, "lower"), vec!["a.md"]);
    assert_eq!(column(&rows, "upper"), vec!["b.MD"]);
}
