//! CLI e2e for `[[dirsql.function]]`: the real `dirsql` binary, a real temp
//! directory, a real python3 worker process, nothing mocked.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! workers shell out to `python3`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn write_worker(dir: &std::path::Path, body: &str) {
    let script = format!(
        r#"
import json
import sys

for line in sys.stdin:
    req = json.loads(line)
    args = req["call"]
    {body}
    sys.stdout.write(json.dumps(resp, separators=(",", ":")) + "\n")
    sys.stdout.flush()
"#
    );
    fs::write(dir.join("worker.py"), script).unwrap();
}

fn run(dir: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .arg("-c")
        .arg(dir.path().join(".dirsql.toml"))
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

#[test]
fn one_shot_query_calls_a_declared_function() {
    let root = TempDir::new().unwrap();
    write_worker(root.path(), r#"resp = {"ok": args[0].upper()}"#);
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let out = run(&root, "SELECT up('hello') AS v");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).expect("stdout must be JSON rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["v"].as_str(), Some("HELLO"));
}

#[test]
fn worker_err_response_fails_the_one_shot_query_with_the_message() {
    let root = TempDir::new().unwrap();
    write_worker(root.path(), r#"resp = {"err": "boom from worker"}"#);
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "boomer"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let out = run(&root, "SELECT boomer('x') AS v");
    assert!(
        !out.status.success(),
        "a worker err response must fail the query"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("boom from worker"), "got stderr: {stderr}");
}

#[test]
fn worker_stderr_passes_through_to_dirsql_stderr() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        r#"sys.stderr.write("progress: embedding...\n")
    sys.stderr.flush()
    resp = {"ok": args[0]}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "ident"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let out = run(&root, "SELECT ident('x') AS v");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("progress: embedding..."),
        "worker stderr must pass through, got stderr: {stderr}"
    );
}
