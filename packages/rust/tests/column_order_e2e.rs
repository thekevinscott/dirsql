//! End-to-end tests for column order: the real `dirsql` binary must print
//! columns in the order the SELECT list names them, in both renderings.
//!
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::process::Output;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("a.md"), "alpha\n").unwrap();
    root
}

fn query(root: &TempDir, sql: &str, format: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
        .args(["--format", format])
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed")
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).expect("stdout must be UTF-8")
}

fn header(out: &Output) -> Vec<String> {
    stdout(out)
        .lines()
        .next()
        .expect("table must have a header line")
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[test]
fn the_table_header_follows_the_select_list() {
    let root = fixture();
    let out = query(&root, "SELECT size, basename FROM './'", "table");

    assert_eq!(header(&out), vec!["size", "basename"]);
}

#[test]
fn reversing_the_select_list_reverses_the_table_header() {
    let root = fixture();
    let out = query(&root, "SELECT basename, size FROM './'", "table");

    assert_eq!(header(&out), vec!["basename", "size"]);
}

#[test]
fn json_keys_follow_the_select_list() {
    let root = fixture();
    let out = query(&root, "SELECT size, basename FROM './'", "json");
    let text = stdout(&out);

    let size = text.find("\"size\"").expect("`size` must be present");
    let basename = text
        .find("\"basename\"")
        .expect("`basename` must be present");
    assert!(
        size < basename,
        "expected `size` before `basename`, got: {text}"
    );
}
