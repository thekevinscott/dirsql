//! End-to-end tests for what a scan does when a table's command fails.
//!
//! These spawn the real compiled `dirsql` binary over a temp directory and
//! assert the three things only the process boundary can show: the exit code,
//! what reaches stdout, and what reaches stderr. Nothing is mocked (real
//! process, real filesystem, real SQLite, real command spawn).
//!
//! The contract under test: a table whose `on-file` command fails is left
//! empty rather than fatal, the scan commits every other table, the failures
//! are named on stderr, and the run exits with a code that says "completed,
//! some tables failed" — distinct from both success and failure, so
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

/// The exit code for "the scan completed, but some tables failed". Distinct
/// from `1` so a caller can separate a partial index from a failed run; `23`
/// follows rsync's "partial transfer due to error".
const PARTIAL: i32 = 23;

/// A hook that exits non-zero when any file it is handed contains `BOOM`, and
/// otherwise emits one row. Kept in a script rather than inline TOML to
/// sidestep nested-quote parsing.
const EXTRACT: &str = "#!/bin/sh\nif grep -q BOOM \"$@\"; then echo \"cannot read $1\" >&2; exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n";

/// A hook that emits a row with an unexpected column alongside a good one.
/// Under `strict = true` that row fails normalization.
const STRICTGEN: &str = "#!/bin/sh\nprintf '[{\"name\":\"ok\"},{\"nope\":1}]'\n";

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
    query_sql(root, "SELECT name FROM items ORDER BY name")
}

fn query_sql(root: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
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
    // The floor: introducing a partial code must not make ordinary runs
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
fn a_failed_table_exits_with_the_partial_code() {
    // Without a distinct code, `dirsql "SELECT …" | jq` cannot tell a complete
    // index from one missing a whole table.
    let root = fixture(EXTRACT, LENIENT_CONFIG);
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let out = query(&root);

    assert_eq!(
        out.status.code(),
        Some(PARTIAL),
        "a scan with a failed table must exit {PARTIAL}, got {out:?}"
    );
    // stdout stays parseable: the table exists and is empty.
    assert_eq!(stdout_rows(&out), json!([]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("items"),
        "the failed table must be named on stderr: {stderr}"
    );
    assert!(
        stderr.contains("cannot read"),
        "the command's stderr must be carried: {stderr}"
    );
}

#[test]
fn a_strict_violation_fails_the_table() {
    // A rejected row is the hook's mistake, so it costs that table alone --
    // aborting here would lose every other table to one bad column.
    let root = fixture(STRICTGEN, STRICT_CONFIG);
    fs::write(root.path().join("a.txt"), "fine\n").unwrap();

    let out = query(&root);

    assert_eq!(
        out.status.code(),
        Some(PARTIAL),
        "a strict violation is one table's problem, not the scan's: {out:?}"
    );
    assert_eq!(
        stdout_rows(&out),
        json!([]),
        "no row of a rejected batch lands"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("items"),
        "the failed table must be named on stderr: {stderr}"
    );
}

#[test]
fn many_failures_are_capped_with_a_count_of_the_rest() {
    // One line per failing table does not scale: a config full of broken
    // hooks should not bury the shell in output.
    let config: String = (0..15)
        .map(|index| {
            format!(
                "[[table]]\nname = \"bad{index:02}\"\nddl = \"CREATE TABLE bad{index:02} (name TEXT)\"\nglob = \"bad{index:02}.txt\"\non-file = \"sh hook.sh\"\n\n"
            )
        })
        .collect();
    let root = fixture(EXTRACT, &config);
    for index in 0..15 {
        fs::write(root.path().join(format!("bad{index:02}.txt")), "BOOM\n").unwrap();
    }

    let out = query_sql(&root, "SELECT name FROM bad00");

    assert_eq!(out.status.code(), Some(PARTIAL), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let named = (0..15)
        .filter(|index| stderr.contains(&format!("`bad{index:02}`")))
        .count();
    assert!(
        named <= 10,
        "at most 10 tables should be named individually, {named} were: {stderr}"
    );
    assert!(
        stderr.contains("and 5 more"),
        "the remainder must be counted, not dropped: {stderr}"
    );
}

#[test]
fn a_sqlite_error_still_fails_the_whole_run() {
    // The split must be real: a hook's failure is per-table, but a broken table
    // definition is not something a partial index can paper over.
    let root = fixture(
        EXTRACT,
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT"
glob = "*.txt"
on-file = "sh hook.sh"
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
