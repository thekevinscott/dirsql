//! End-to-end: the real `dirsql` binary resolves a key repeated within one
//! hook row the same way through `--on-file` and a `[[table]]`: the last
//! value wins. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn query(args: &[&str]) -> String {
    let root = TempDir::new().unwrap();
    std::fs::write(
        root.path().join("d.ndjson"),
        "{\"k\":\"first\",\"k\":\"second\"}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("c.toml"),
        "[[table]]\nname = \"dup\"\nddl = \"CREATE TABLE dup (k TEXT)\"\nglob = \"d.ndjson\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .args(args)
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
fn on_file_flag_keeps_the_last_value_of_a_repeated_key() {
    assert_eq!(
        query(&["SELECT k FROM './d.ndjson'", "--on-file", "cat"]),
        "[{\"k\":\"second\"}]\n"
    );
}

#[test]
fn a_config_table_keeps_the_last_value_of_a_repeated_key() {
    assert_eq!(
        query(&["SELECT k FROM dup", "-c", "c.toml"]),
        "[{\"k\":\"second\"}]\n"
    );
}
