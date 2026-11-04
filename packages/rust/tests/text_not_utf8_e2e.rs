//! End-to-end tests for text SQLite holds as invalid UTF-8: the real
//! `dirsql` binary must fail the query with an error naming the column, in
//! every format, never crash or print replacement characters.
//!
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn failing(sql: &str, format: &str) -> String {
    let root = TempDir::new().unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
        .args(["--format", format])
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "expected failure, stdout: {stdout}");
    assert!(!stdout.contains('\u{FFFD}'), "stdout: {stdout}");
    String::from_utf8(out.stderr).expect("stderr must be UTF-8")
}

fn assert_names_the_column(stderr: &str) {
    assert!(
        stderr.contains("column \"t\" holds text that is not valid UTF-8"),
        "stderr: {stderr}"
    );
    assert!(!stderr.contains("panicked"), "stderr: {stderr}");
}

#[test]
fn table_format_fails_on_text_that_is_not_utf8() {
    assert_names_the_column(&failing("SELECT CAST(x'ff41' AS TEXT) AS t", "table"));
}

#[test]
fn json_format_fails_on_text_that_is_not_utf8() {
    assert_names_the_column(&failing("SELECT CAST(x'ff41' AS TEXT) AS t", "json"));
}

#[test]
fn a_lone_surrogate_fails_the_query() {
    assert_names_the_column(&failing("SELECT char(55296) AS t", "table"));
}
