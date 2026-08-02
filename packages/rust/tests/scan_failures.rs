//! Integration tests for the scan's record of files it skipped.
//!
//! A hook failure is per-file, so a caller needs to know *which* files were
//! skipped rather than inferring a partial index from absent rows. These build
//! a real `DirSQL` over real temp files (the effectful spawn path, kept out of
//! colocated unit tests by the Rust isolation rule).
//!
//! Unix-only: the fixtures shell out to `sh`. The Rust CI test job runs on Linux.
#![cfg(unix)]

use std::fs;

use dirsql::DirSQL;
use tempfile::TempDir;

/// The skipped files are reachable from the built database, so a caller can
/// report them rather than inferring a partial index from missing rows.
#[test]
fn skipped_files_are_reported_on_the_built_database() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$1\"; then exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh {path}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();

    let skipped = db.scan_failures();
    assert_eq!(skipped.len(), 1, "expected one skipped file: {skipped:?}");
    assert!(
        skipped[0].path.contains("bad.txt"),
        "the failure names the file: {skipped:?}"
    );
}

/// A clean scan reports nothing, so a caller can use emptiness as the signal.
#[test]
fn a_clean_scan_reports_no_skipped_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "printf '[{\"name\":\"ok\"}]'"
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
