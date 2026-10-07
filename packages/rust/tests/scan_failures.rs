//! Integration test for the scan's record of the files it could not index.
//!
//! A clean scan over a config-declared table must report nothing, so a caller
//! can use emptiness as the signal. This builds a real `DirSQL` over real temp
//! files (the effectful spawn path, kept out of colocated unit tests by the
//! Rust isolation rule).
//!
//! Unix-only: the fixture shells out to `printf`. The Rust CI test job runs on
//! Linux.
#![cfg(unix)]

use std::fs;

use dirsql::DirSQL;
use tempfile::TempDir;

#[test]
fn a_clean_scan_reports_no_skipped_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "printf '{\"name\":\"ok\"}'"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();

    assert!(db.scan_failures().is_empty());
}
