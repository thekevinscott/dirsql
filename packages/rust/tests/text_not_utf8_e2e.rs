//! End-to-end tests for text SQLite holds as invalid UTF-8: the real
//! `dirsql` binary must print it with replacement characters in every
//! format, never crash.
//!
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn query(sql: &str, format: &str) -> String {
    let root = TempDir::new().unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
        .args(["--format", format])
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
fn table_format_prints_text_that_is_not_utf8_with_replacement_characters() {
    let out = query("SELECT CAST(x'ff41' AS TEXT) AS t", "table");
    assert!(out.contains("\u{FFFD}A"), "stdout: {out}");
}

#[test]
fn table_format_prints_a_lone_surrogate_with_a_replacement_character() {
    let out = query("SELECT char(55296) AS t", "table");
    assert!(out.contains('\u{FFFD}'), "stdout: {out}");
}
