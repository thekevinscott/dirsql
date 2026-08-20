//! End-to-end tests for progress on worker-backed function calls.
//!
//! A query that calls a `[[dirsql.function]]` pays one worker round trip per
//! row (`docs/reference/config.md`, "Worker lifecycle"). On a corpus-sized
//! query — `dirsql-plugin-embeddings` runs `embed(content)` over every matched
//! file — that is tens of thousands of round trips inside a single `query()`
//! call, and dirsql#957 is that they happened in silence.
//!
//! These spawn the real `dirsql` binary against a real python3 worker with
//! nothing mocked, and pin the property that made worker-side reporting
//! unworkable (dirsql#1001): the counter is drawn and **erased by the process
//! that owns stdout**, so the query result is never glued onto a leftover
//! progress line. A worker drawing its own bar cannot do that — it is
//! SIGKILLed, and it is killed after the result is printed anyway.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! worker shells out to `python3`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// A worker that answers every call by echoing its argument back, one round
/// trip per call — the shape every worker-backed function has.
const WORKER: &str = r#"
import json
import sys

for line in sys.stdin:
    req = json.loads(line)
    sys.stdout.write(json.dumps({"ok": req["call"][0]}, separators=(",", ":")) + "\n")
    sys.stdout.flush()
"#;

const CONFIG: &str = r#"
[[dirsql.function]]
name = "echo"
args = [1]
command = "python3 worker.py"
"#;

/// A root with `count` files and a config declaring the worker function, so a
/// query over the path-table makes exactly `count` round trips.
fn fixture(count: usize) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("worker.py"), WORKER).unwrap();
    fs::write(root.path().join(".dirsql.toml"), CONFIG).unwrap();
    for i in 0..count {
        fs::write(root.path().join(format!("file{i}.txt")), "x\n").unwrap();
    }
    root
}

fn run(dir: &std::path::Path, sql: &str, progress: Option<&str>) -> Output {
    let mut cmd = std::process::Command::cargo_bin("dirsql").expect("binary must exist");
    cmd.arg("query")
        .arg(sql)
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

/// The headline: a query paying per-row worker round trips says so, rather
/// than sitting silent for however long the worker takes.
#[test]
fn forced_progress_counts_the_worker_round_trips() {
    let root = fixture(4);

    let out = run(
        root.path(),
        "SELECT echo(basename) AS v FROM './*.txt'",
        Some("always"),
    );

    let err = stderr(&out);
    assert!(
        err.contains("worker calls"),
        "the round trips are counted on stderr: {err:?}"
    );
}

/// What survives the query is one line naming what it cost.
#[test]
fn forced_progress_summarizes_the_round_trips() {
    let root = fixture(4);

    let out = run(
        root.path(),
        "SELECT echo(basename) AS v FROM './*.txt'",
        Some("always"),
    );

    let err = stderr(&out);
    assert!(
        err.contains("dirsql: ran 4 worker calls in "),
        "the summary carries the count and the elapsed time: {err:?}"
    );
}

/// The regression this whole design exists for (dirsql#1001): the live line is
/// erased before the result is printed, so stdout is parseable JSON and not
/// `dirsql: running 4 worker calls[{"v":…}]`.
#[test]
fn the_result_is_never_glued_onto_a_leftover_progress_line() {
    let root = fixture(4);

    let out = run(
        root.path(),
        "SELECT echo(basename) AS v FROM './*.txt'",
        Some("always"),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let rows: Vec<Value> = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is JSON rows: {e}: {stdout:?}"));
    assert_eq!(rows.len(), 4, "every file is still returned: {rows:?}");
    assert!(
        !stdout.contains("worker calls"),
        "progress never reaches stdout: {stdout:?}"
    );
}

/// A query that calls no worker has nothing to report, even forced on: the
/// reporter speaks only when there is something to say.
#[test]
fn a_query_with_no_worker_calls_reports_nothing() {
    let root = fixture(4);

    let out = run(root.path(), "SELECT 1 AS v", Some("always"));

    let err = stderr(&out);
    assert!(
        !err.contains("worker calls"),
        "no round trips means no counter: {err:?}"
    );
}

/// The default is `auto`, so a piped run stays silent however many round trips
/// it pays for.
#[test]
fn worker_call_progress_is_silent_when_stderr_is_not_a_terminal() {
    let root = fixture(4);

    let out = run(
        root.path(),
        "SELECT echo(basename) AS v FROM './*.txt'",
        None,
    );

    let err = stderr(&out);
    assert_eq!(err, "", "a piped run writes nothing to stderr: {err:?}");
}
