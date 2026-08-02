//! End-to-end tests for what a scan does when individual files fail.
//!
//! These spawn the real compiled `dirsql` binary over a temp directory and
//! assert the three things only the process boundary can show: the exit code,
//! what reaches stdout, and what reaches stderr. Nothing is mocked (real
//! process, real filesystem, real SQLite, real command spawn).
//!
//! The contract under test (dirsql#714): a file whose `on-file` hook fails is
//! skipped rather than fatal, the scan commits what it could index, the skips
//! are named on stderr, and the run exits with a code that says "completed,
//! some files skipped" — distinct from both success and failure, so
//! `dirsql "SELECT …" | jq` under `set -e` can tell a partial index from a
//! broken run.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixtures shell out to `sh`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

/// The exit code for "the scan completed, but some files were skipped".
/// Distinct from `1` so a caller can separate a partial index from a failed
/// run; `23` follows rsync's "partial transfer due to error".
const PARTIAL: i32 = 23;

/// A hook that exits non-zero for any file containing `BOOM`, and otherwise
/// emits one row. Kept in a script rather than inline TOML to sidestep
/// nested-quote parsing.
const EXTRACT: &str = "#!/bin/sh\nif grep -q BOOM \"$1\"; then echo \"cannot read $1\" >&2; exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n";

/// A hook that emits an unexpected column for any file containing `BAD`. Under
/// `strict = true` that row fails normalization.
const STRICTGEN: &str = "#!/bin/sh\nif grep -q BAD \"$1\"; then printf '[{\"nope\":1}]'; else printf '[{\"name\":\"ok\"}]'; fi\n";

fn fixture(script: &str, config: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("hook.sh"), script).unwrap();
    fs::write(root.path().join(".dirsql.toml"), config).unwrap();
    root
}

const LENIENT_CONFIG: &str = r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh hook.sh {path}"
"#;

const STRICT_CONFIG: &str = r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
strict = true
on-file = "sh hook.sh {path}"
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

fn stdout_rows(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout was not JSON ({e}): {:?}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

#[test]
fn a_scan_with_no_failures_still_exits_zero() {
    // The floor: introducing a skipped-files code must not make ordinary runs
    // non-zero.
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let out = query(&root);

    assert_eq!(
        out.status.code(),
        Some(0),
        "a clean scan exits 0, got {out:?}"
    );
    assert_eq!(stdout_rows(&out), json!([{"name": "ok"}]));
}

#[test]
fn a_skipped_file_exits_with_the_partial_code() {
    // Without a distinct code, `dirsql "SELECT …" | jq` cannot tell a complete
    // index from one missing half its files.
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let out = query(&root);

    assert_eq!(
        out.status.code(),
        Some(PARTIAL),
        "a scan that skipped a file must exit {PARTIAL}, got {out:?}"
    );
    // stdout stays parseable: the good file's row is there and nothing else.
    assert_eq!(stdout_rows(&out), json!([{"name": "ok"}]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bad.txt"),
        "the skipped file must be named on stderr: {stderr}"
    );
}

#[test]
fn a_strict_violation_skips_only_that_file() {
    // A rejected row is the hook's mistake, so it costs that file alone --
    // aborting here would lose every other file's rows to one bad column.
    let root = fixture(STRICTGEN, STRICT_CONFIG);
    fs::write(root.path().join("a_good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("z_bad.txt"), "BAD\n").unwrap();

    let out = query(&root);

    assert_eq!(
        out.status.code(),
        Some(PARTIAL),
        "a strict violation is one file's problem, not the scan's: {out:?}"
    );
    assert_eq!(
        stdout_rows(&out),
        json!([{"name": "ok"}]),
        "the well-formed file must still be indexed"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("z_bad.txt"),
        "the skipped file must be named on stderr: {stderr}"
    );
}

#[test]
fn many_failures_are_capped_with_a_count_of_the_rest() {
    // One line per failing file does not scale: a directory of unreadable
    // files should not bury the shell in output.
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    for index in 0..15 {
        fs::write(root.path().join(format!("bad{index:02}.txt")), "BOOM\n").unwrap();
    }

    let out = query(&root);

    assert_eq!(out.status.code(), Some(PARTIAL), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let named = (0..15)
        .filter(|index| stderr.contains(&format!("bad{index:02}.txt")))
        .count();
    assert!(
        named <= 10,
        "at most 10 files should be named individually, {named} were: {stderr}"
    );
    assert!(
        stderr.contains("and 5 more"),
        "the remainder must be counted, not dropped: {stderr}"
    );
}

#[test]
fn a_sqlite_error_still_fails_the_whole_run() {
    // The split must be real: a hook's failure is per-file, but a broken table
    // definition is not something a partial index can paper over.
    let root = fixture(
        EXTRACT,
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT"
glob = "*.txt"
on-file = "sh hook.sh {path}"
"#,
    );
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let out = query(&root);

    let code = out.status.code();
    assert_ne!(code, Some(0), "a malformed DDL must not pass: {out:?}");
    assert_ne!(
        code,
        Some(PARTIAL),
        "a DDL error is a failed run, not a partial one: {out:?}"
    );
}
