//! End-to-end: a REPL session rejects `BEGIN`, `SAVEPOINT` and `PRAGMA`
//! setters, so a statement returns the same rows inside a session as in a
//! fresh process. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Output, Stdio};

use assert_cmd::prelude::*;
use tempfile::TempDir;

const LIKE: &str = "SELECT count(*) AS n FROM './' WHERE basename LIKE 'A.MD'";

fn repl(stdin: &str) -> Output {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("a.md"), "a\n").unwrap();
    let mut child = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
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
        .expect("writing to the REPL's stdin failed");
    child
        .wait_with_output()
        .expect("waiting on `dirsql` failed")
}

#[test]
fn a_pragma_setter_is_rejected_and_does_not_change_later_rows() {
    let out = repl(&format!("PRAGMA case_sensitive_like=1\n{LIKE}\n"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stdout, "[{\"n\":1}]\n", "stderr: {stderr}");
    assert!(stderr.contains("not authorized"), "stderr: {stderr}");
}

#[test]
fn transaction_control_is_rejected() {
    let out = repl("BEGIN\nSAVEPOINT a\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(stdout, "", "stderr: {stderr}");
    assert_eq!(
        stderr.matches("not authorized").count(),
        2,
        "stderr: {stderr}"
    );
}
