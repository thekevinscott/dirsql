//! A file modified before 1970 reports `mtime` as negative Unix seconds,
//! not NULL. Real filesystem, real SQLite, SDK public API.

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

#[test]
fn a_pre_epoch_mtime_is_negative_seconds() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("neg.md");
    fs::write(&file, "x").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(UNIX_EPOCH - Duration::from_secs(86_400))
        .unwrap();
    let db = DirSQL::builder().root(root.path()).build().unwrap();

    let rows = db.query("SELECT mtime FROM './neg.md'").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("mtime"), Some(&Value::Integer(-86_400)));
}
