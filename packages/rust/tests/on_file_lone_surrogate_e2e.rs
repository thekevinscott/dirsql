//! End-to-end: a lone surrogate escape in `--on-file` output fails the query
//! through the real `dirsql` binary with an error naming the line and the
//! escape. Nothing is mocked.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

#[test]
fn a_lone_surrogate_fails_with_the_line_and_the_escape() {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("s.ndjson"), "{\"a\":\"\\ud800\"}\n").unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args([
            "query",
            "SELECT typeof(a) FROM './s.ndjson'",
            "--on-file",
            "cat",
            "--format",
            "json",
        ])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(out.stdout.is_empty(), "stdout: {:?}", out.stdout);
    assert!(
        stderr.contains(r"line 1: lone surrogate \ud800 is not valid Unicode"),
        "stderr: {stderr}"
    );
}
