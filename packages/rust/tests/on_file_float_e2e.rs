//! End-to-end: a float an `on-file` hook prints reaches SQLite as the same
//! REAL a correctly rounded JSON parser reads, through the real `dirsql`
//! binary. Nothing is mocked.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

#[test]
fn an_on_file_float_round_trips_exactly() {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("a.ndjson"), "{\"v\":123456789.12345679}\n").unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args([
            "query",
            "SELECT v, v = 123456789.12345679 AS eq FROM './a.ndjson'",
            "--on-file",
            "cat",
            "--format",
            "json",
        ])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "[{\"v\":123456789.12345679,\"eq\":1}]\n"
    );
}
