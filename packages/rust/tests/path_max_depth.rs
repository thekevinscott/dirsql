//! Integration tests: a file whose absolute path is longer than the
//! platform's path limit is still a row of a path table, with its stat
//! columns and content. Real filesystem, real SQLite, SDK public API.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

const LEVELS: i64 = 30;

/// `LEVELS` nested directories with 200-character names, one file in each,
/// so the deepest files sit far past a 4096-byte absolute path. Built from
/// the bottom up, renaming the chain so far into each new parent, because no
/// call may name a path past the limit.
fn deep_tree() -> TempDir {
    let staging = TempDir::new().unwrap();
    let name = "D".repeat(200);
    let chain = staging.path().join("chain");
    let parent = staging.path().join("parent");
    for level in (1..=LEVELS).rev() {
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join(format!("leaf{level:02}.md")), "x").unwrap();
        if level < LEVELS {
            fs::rename(&chain, parent.join(&name)).unwrap();
        }
        fs::rename(&parent, &chain).unwrap();
    }
    let root = TempDir::new().unwrap();
    fs::rename(&chain, root.path().join(&name)).unwrap();
    root
}

#[test]
fn a_double_star_keeps_files_past_the_path_limit() {
    let root = deep_tree();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    let rows = db
        .query("SELECT COUNT(*) n, COUNT(size) sized, COUNT(content) read FROM './**'")
        .unwrap();
    let expected = Value::Integer(LEVELS);
    assert_eq!(rows[0]["n"], expected);
    assert_eq!(rows[0]["sized"], expected);
    assert_eq!(rows[0]["read"], expected);
}
