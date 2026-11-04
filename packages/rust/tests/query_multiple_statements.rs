//! Integration tests: `query()` runs exactly one statement and rejects any SQL
//! after it, rather than returning the first statement's rows and dropping the
//! rest unseen.

use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn items_db(root: &Path) -> DirSQL {
    fs::write(root.join("a.txt"), "apple").unwrap();
    DirSQL::new(
        root,
        vec![Table::new(
            "items",
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
fn query_rejects_sql_after_the_first_statement() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    for sql in [
        "SELECT 1 AS a; SELECT 2 AS b",
        "SELECT 1 AS a; SELECT 2 AS b;",
        "SELECT name FROM items; garbage here",
        "SELECT 1 AS a; SELECT * FROM no_such_table",
        "SELECT 1 AS a;; SELECT 2 AS b",
        "SELECT 1 AS a; -- c\nSELECT 2 AS b",
    ] {
        let err = db
            .query(sql)
            .expect_err(&format!("{sql:?} must be rejected"));
        assert!(
            err.to_string().contains("one statement"),
            "{sql:?} should be rejected as more than one statement, got: {err}"
        );
    }
}

#[test]
fn query_accepts_a_statement_followed_only_by_terminators_and_comments() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    for sql in [
        "SELECT name FROM items",
        "SELECT name FROM items;",
        "SELECT name FROM items; ",
        "SELECT name FROM items;;\n",
        "SELECT name FROM items; -- trailing comment",
        "SELECT name FROM items; /* block */",
        "-- leading comment\nSELECT name FROM items",
    ] {
        let rows = db
            .query(sql)
            .unwrap_or_else(|e| panic!("{sql:?} must run, got: {e}"));
        assert_eq!(rows.len(), 1, "{sql:?}");
        assert_eq!(rows[0]["name"], Value::Text("apple".into()), "{sql:?}");
    }
}

#[test]
fn a_semicolon_inside_a_literal_does_not_end_the_statement() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    let rows = db.query("SELECT 'x; SELECT 2' AS s").unwrap();
    assert_eq!(rows[0]["s"], Value::Text("x; SELECT 2".into()));
}
