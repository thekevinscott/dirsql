//! End-to-end tests for what a run does when a table's command fails.
//!
//! These spawn the real compiled `dirsql` binary over a temp directory and
//! assert the three things only the process boundary can show: the exit code,
//! what reaches stdout, and what reaches stderr. Nothing is mocked (real
//! process, real filesystem, real SQLite, real command spawn).
//!
//! The contract under test: an `on-file` command runs once for its whole
//! table, so its failure is the table's and the table's failure is the run's.
//! The failed table is named on stderr with the command's own stderr, no rows
//! reach stdout, and the run exits non-zero like any other failed build.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixtures shell out to `sh`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::json;
use tempfile::TempDir;

/// A hook that exits non-zero when any file it is handed contains `BOOM`, and
/// otherwise emits one row. Kept in a script rather than inline TOML to
/// sidestep nested-quote parsing.
const EXTRACT: &str = "#!/bin/sh\nif grep -q BOOM \"$@\"; then echo \"cannot read $1\" >&2; exit 1; fi\nprintf '{\"name\":\"ok\"}'\n";

/// A hook that emits a row with an unexpected column alongside a good one.
/// Under `strict = true` that row fails normalization.
const STRICTGEN: &str = "#!/bin/sh\nprintf '{\"name\":\"ok\"}\\n{\"nope\":1}'\n";

fn fixture(script: &str, config: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("hook.sh"), script).unwrap();
    fs::write(root.path().join(".dirsql.toml"), config).unwrap();
    root
}

const LENIENT_CONFIG: &str = r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh hook.sh"
"#;

const STRICT_CONFIG: &str = r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
strict = true
on-file = "sh hook.sh"
"#;

fn query(root: &TempDir) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("SELECT name FROM items ORDER BY name")
        .arg("-c")
        .arg(".dirsql.toml")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed")
}

#[test]
fn a_clean_scan_exits_zero() {
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let out = query(&root);

    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&out.stdout).unwrap(),
        json!([{"name": "ok"}])
    );
}

#[test]
fn a_failed_command_fails_the_run() {
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let out = query(&root);

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "no rows reach stdout: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`items`"),
        "the failed table must be named on stderr: {stderr}"
    );
    assert!(
        stderr.contains("cannot read"),
        "the command's stderr must be carried: {stderr}"
    );
}

#[test]
fn a_strict_violation_fails_the_run() {
    let root = fixture(STRICTGEN, STRICT_CONFIG);
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let out = query(&root);

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty(), "no rows reach stdout: {out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("`items`"),
        "the failed table must be named on stderr: {stderr}"
    );
}
