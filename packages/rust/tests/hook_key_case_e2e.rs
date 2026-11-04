//! End-to-end: the real `dirsql` binary fills a declared column from an
//! `on-file` key that differs from it only by case. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn query(strict: bool) -> String {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("t.ndjson"), "{\"title\":\"hello\"}\n").unwrap();
    std::fs::write(
        root.path().join("c.toml"),
        format!(
            "[[table]]\nname = \"posts\"\nddl = \"CREATE TABLE posts (TITLE TEXT)\"\nglob = \"t.ndjson\"\nstrict = {strict}\non-file = \"cat\"\n"
        ),
    )
    .unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(["query", "SELECT title FROM posts", "-c", "c.toml"])
        .args(["--format", "json"])
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
fn a_key_differing_from_its_column_only_by_case_fills_that_column() {
    assert_eq!(query(false), "[{\"TITLE\":\"hello\"}]\n");
}

#[test]
fn a_strict_table_accepts_a_key_differing_from_its_column_only_by_case() {
    assert_eq!(query(true), "[{\"TITLE\":\"hello\"}]\n");
}
