//! End-to-end: the CLI rejects SQL after the first statement instead of
//! printing the first statement's rows and exiting 0. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Output, Stdio};

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn dirsql(args: &[&str], stdin: &str) -> Output {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("a.md"), "a\n").unwrap();
    let mut child = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .args(["--format", "json"])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning `dirsql` failed");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes())
        .expect("writing to stdin failed");
    child
        .wait_with_output()
        .expect("waiting on `dirsql` failed")
}

#[test]
fn a_one_shot_query_with_a_second_statement_fails() {
    for args in [
        vec!["SELECT 1 AS a; SELECT 2 AS b"],
        vec!["query", "SELECT 1 AS a; garbage here"],
    ] {
        let out = dirsql(&args, "");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "{args:?} must fail, stderr: {stderr}"
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), "", "{args:?}");
        assert!(stderr.contains("one statement"), "{args:?}: {stderr}");
    }
}

#[test]
fn a_one_shot_query_with_a_trailing_semicolon_and_comment_runs() {
    let out = dirsql(&["SELECT 1 AS a; -- done"], "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "[{\"a\":1}]\n");
}

#[test]
fn a_piped_line_with_two_statements_is_rejected_and_the_session_continues() {
    let out = dirsql(&[], "SELECT 1 AS a; SELECT 2 AS b;\nSELECT 3 AS c\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stdout, "[{\"c\":3}]\n", "stderr: {stderr}");
    assert!(stderr.contains("one statement"), "stderr: {stderr}");
}
