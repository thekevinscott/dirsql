//! Integration tests for the scan's record of the tables it could not fill.
//!
//! A hook failure is per-table, so a caller needs to know *which* tables are
//! empty because their command failed rather than inferring it from absent
//! rows. These build a real `DirSQL` over real temp files (the effectful
//! spawn path, kept out of colocated unit tests by the Rust isolation rule).
//!
//! Unix-only: the fixtures shell out to `sh`. The Rust CI test job runs on Linux.
#![cfg(unix)]

use std::fs;

use dirsql::DirSQL;
use tempfile::TempDir;

/// A failed table is reachable from the built database under its name, with
/// the command's stderr, so a caller can report it rather than inferring an
/// empty table from missing rows.
#[test]
fn failed_tables_are_reported_on_the_built_database() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$@\"; then echo 'cannot read' >&2; exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh"
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

    let failed = db.scan_failures();
    assert_eq!(failed.len(), 1, "expected one failed table: {failed:?}");
    assert_eq!(failed[0].path, "items", "the failure names the table");
    assert!(
        failed[0].message.contains("cannot read"),
        "the failure carries the command's stderr: {failed:?}"
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
name = "items"
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
