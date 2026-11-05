//! CLI e2e for a file longer than SQLite's default string limit of 1e9
//! bytes: the real `dirsql` binary must answer the query with that file's
//! `content` NULL, not fail it. Nothing is mocked.
//!
//! The file is sparse (`set_len`), so the fixture costs no disk writes.

#![cfg(feature = "cli")]

use std::fs::{self, File};

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

#[test]
fn a_file_over_the_sqlite_length_limit_reads_as_null_content() {
    let root = TempDir::new().unwrap();
    File::create(root.path().join("big.txt"))
        .unwrap()
        .set_len(1_000_000_001)
        .unwrap();
    fs::write(root.path().join("small.txt"), "small").unwrap();

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("SELECT basename, typeof(content) AS t FROM './*' ORDER BY basename")
        .args(["--format", "json"])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        rows,
        json!([
            {"basename": "big.txt", "t": "null"},
            {"basename": "small.txt", "t": "text"},
        ])
    );
}
