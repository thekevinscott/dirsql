//! A bare `-0` in an `on-file` hook's output is an integral number, so it is
//! stored as INTEGER 0, as SQLite's own JSON functions type it.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

#[test]
fn a_negative_zero_is_stored_as_integer_zero() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (z, f)\"\nglob = \"*.ndjson\"\non-file = \"cat\"\n",
    )
    .unwrap();
    fs::write(root.path().join("s.ndjson"), "{\"z\":-0,\"f\":-0.0}\n").unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db
        .query("SELECT z, typeof(z) AS zt, typeof(f) AS ft FROM t")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["z"], Value::Integer(0));
    assert_eq!(rows[0]["zt"], Value::Text("integer".into()));
    assert_eq!(rows[0]["ft"], Value::Text("real".into()));
}
