//! Integration tests: a parsed path-table infers one column for keys that
//! differ only in case, since SQLite identifiers are ASCII-case-insensitive
//! and two such columns could not both be declared.

use std::fs;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

fn query(ndjson: &str, sql: &str) -> Vec<Row> {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("c.ndjson"), ndjson).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .path_table_parser("cat")
        .build()
        .unwrap();
    db.query(sql).unwrap()
}

#[test]
fn keys_differing_only_in_case_across_rows_fill_one_column() {
    let rows = query(
        "{\"title\":\"lower\"}\n{\"Title\":\"upper\"}\n",
        "SELECT * FROM './c.ndjson'",
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].len(), 1, "{rows:?}");
    assert_eq!(rows[0]["title"], Value::Text("lower".into()));
    assert_eq!(rows[1]["title"], Value::Text("upper".into()));
}

#[test]
fn keys_differing_only_in_case_within_one_row_fill_one_column() {
    let rows = query(
        "{\"title\":\"lower\",\"Title\":\"upper\"}\n",
        "SELECT COUNT(*) AS n FROM './c.ndjson'",
    );
    assert_eq!(rows[0]["n"], Value::Integer(1));
}
