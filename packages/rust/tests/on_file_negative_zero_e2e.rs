//! End-to-end: a bare `-0` in `--on-file` output reads back through the real
//! `dirsql` binary as INTEGER 0. Nothing is mocked.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

#[test]
fn a_negative_zero_reads_back_as_integer_zero() {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("z.ndjson"), "{\"e\":-0}\n").unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args([
            "query",
            "SELECT e, typeof(e) t FROM './z.ndjson'",
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
    assert!(out.status.success(), "stderr: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        r#"[{"e":0,"t":"integer"}]"#
    );
}
