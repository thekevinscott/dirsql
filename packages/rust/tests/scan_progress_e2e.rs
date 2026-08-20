//! End-to-end tests for progress reporting on the startup scan.
//!
//! A cold scan over a large corpus can run for minutes with nothing on the
//! terminal (dirsql#957). These spawn the real compiled `dirsql` binary over a
//! temp directory and assert the two things only the process boundary can
//! show: what reaches stderr while the index is built, and that stdout stays
//! the query result alone. Nothing is mocked.
//!
//! The gate is `DIRSQL_PROGRESS`: `auto` (the default) draws only on a
//! terminal, so a piped run -- which is what these tests capture, and what a
//! `| jq` pipeline is -- must stay byte-for-byte silent. `always` forces the
//! reporting on regardless, which is what makes the drawn output assertable
//! here without a pty.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixtures shell out to `sh`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const CONFIG: &str = r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = '''sh -c 'printf "[{\"name\":\"%s\"}]" "${1##*/}"' sh {path}'''
"#;

/// A root with `count` matching files and a config that indexes them.
fn fixture(count: usize) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join(".dirsql.toml"), CONFIG).unwrap();
    for i in 0..count {
        fs::write(root.path().join(format!("file{i}.txt")), "x\n").unwrap();
    }
    root
}

/// Run `dirsql -c .dirsql.toml query <sql>` with an explicit `DIRSQL_PROGRESS`
/// setting (or none), capturing both streams.
fn run(dir: &std::path::Path, progress: Option<&str>) -> Output {
    let mut cmd = std::process::Command::cargo_bin("dirsql").expect("binary must exist");
    cmd.arg("query")
        .arg("SELECT count(*) AS n FROM items")
        .arg("-c")
        .arg(".dirsql.toml")
        .current_dir(dir)
        .env_remove("DIRSQL_PROGRESS");
    if let Some(value) = progress {
        cmd.env("DIRSQL_PROGRESS", value);
    }
    cmd.output().expect("spawning `dirsql query` failed")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The headline: with reporting forced on, the ingest phase names how many
/// files it has indexed out of how many it found, so a user watching a long
/// scan can see it moving.
#[test]
fn forced_progress_reports_the_ingest_phase_on_stderr() {
    let root = fixture(3);

    let out = run(root.path(), Some("always"));

    let err = stderr(&out);
    assert!(
        err.contains("dirsql: indexing"),
        "the ingest phase draws a labeled progress line: {err:?}"
    );
    assert!(
        err.contains("/3 files"),
        "the line carries the scan's total: {err:?}"
    );
}

/// The walk runs before anything can be counted against a total, so it reports
/// a running count of the files it has found.
#[test]
fn forced_progress_reports_the_walk_phase_on_stderr() {
    let root = fixture(3);

    let out = run(root.path(), Some("always"));

    let err = stderr(&out);
    assert!(
        err.contains("dirsql: scanning"),
        "the directory walk draws a labeled progress line: {err:?}"
    );
}

/// Progress is erased when the phase ends, so what survives is one summary
/// line saying what the run cost -- the point of showing it at all.
#[test]
fn forced_progress_leaves_a_summary_of_what_the_scan_cost() {
    let root = fixture(3);

    let out = run(root.path(), Some("always"));

    let err = stderr(&out);
    assert!(
        err.contains("dirsql: indexed 3 files in "),
        "the ingest phase summarizes its file count and elapsed time: {err:?}"
    );
}

/// stdout is the query result alone: progress never contaminates the stream a
/// `| jq` pipeline reads.
#[test]
fn forced_progress_never_touches_stdout() {
    let root = fixture(3);

    let out = run(root.path(), Some("always"));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| panic!("stdout is JSON: {e}: {stdout:?}"));
    assert_eq!(value[0]["n"], 3, "the query still returns its rows");
}

/// The default is `auto`, which means "a terminal or nothing". These tests
/// capture stderr through a pipe, so the run must be silent -- the property
/// that keeps progress out of logs, CI output and `2>` redirects.
#[test]
fn progress_is_silent_by_default_when_stderr_is_not_a_terminal() {
    let root = fixture(3);

    let out = run(root.path(), None);

    let err = stderr(&out);
    assert_eq!(err, "", "a piped run writes nothing to stderr: {err:?}");
}

/// `never` is the opt-out an embedder sets to guarantee silence even on a
/// terminal.
#[test]
fn progress_is_silent_when_explicitly_disabled() {
    let root = fixture(3);

    let out = run(root.path(), Some("never"));

    let err = stderr(&out);
    assert_eq!(err, "", "an opted-out run writes nothing to stderr: {err:?}");
}
