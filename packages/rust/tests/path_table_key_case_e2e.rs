//! End-to-end: the real `dirsql` binary queries `--on-file` output whose keys
//! differ only in case, inferring one column for them. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn query(ndjson: &str, sql: &str) -> String {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("c.ndjson"), ndjson).unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(["query", sql, "--on-file", "cat", "--format", "json"])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout must be UTF-8")
}

#[test]
fn keys_differing_only_in_case_across_rows_fill_one_column() {
    assert_eq!(
        query(
            "{\"title\":\"lower\"}\n{\"Title\":\"upper\"}\n",
            "SELECT * FROM './c.ndjson'"
        ),
        "[{\"title\":\"lower\"},{\"title\":\"upper\"}]\n"
    );
}

#[test]
fn keys_differing_only_in_case_within_one_row_fill_one_column() {
    assert_eq!(
        query(
            "{\"title\":\"lower\",\"Title\":\"upper\"}\n",
            "SELECT COUNT(*) AS n FROM './c.ndjson'"
        ),
        "[{\"n\":1}]\n"
    );
}
