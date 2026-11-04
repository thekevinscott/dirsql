//! A float an `on-file` hook prints is stored as the REAL a correctly
//! rounded JSON parser reads.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

#[test]
fn a_real_column_keeps_the_exact_double() {
    let doubles = [
        "123456789.12345679",
        "0.30000000000000004",
        "1.7976931348623157e308",
    ];
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (v REAL)\"\nglob = \"*.ndjson\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let lines: String = doubles.iter().map(|d| format!("{{\"v\":{d}}}\n")).collect();
    fs::write(root.path().join("a.ndjson"), lines).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let stored: Vec<Value> = db
        .query("SELECT v FROM t ORDER BY rowid")
        .unwrap()
        .into_iter()
        .map(|row| row["v"].clone())
        .collect();
    let expected: Vec<Value> = doubles
        .iter()
        .map(|d| Value::Real(d.parse().unwrap()))
        .collect();
    assert_eq!(stored, expected);
}
