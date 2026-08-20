//! End-to-end tests for the declared `[[table]] name` key.
//!
//! Spawns the real compiled `dirsql` binary against a real temp directory and
//! a real `.dirsql.toml`. Nothing is mocked (real process, real filesystem,
//! real SQLite, real `on-file` command spawn).
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixture shells out to `cat`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// A tempdir holding one JSON file of rows and a `.dirsql.toml` with `config`.
fn fixture(config: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(
        root.path().join("data/a.json"),
        r#"[{"id": "one"}, {"id": "two"}]"#,
    )
    .unwrap();
    fs::write(root.path().join(".dirsql.toml"), config).unwrap();
    root
}

fn query(root: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("query")
        .arg(sql)
        .arg("--config")
        .arg(root.path().join(".dirsql.toml"))
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

#[test]
fn declared_name_is_queryable_through_the_cli() {
    let root = fixture(
        r#"
[[table]]
name = "records"
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
"#,
    );

    let out = query(&root, "SELECT id FROM records ORDER BY id");
    assert!(
        out.status.success(),
        "a [[table]] with a matching `name` must query cleanly, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).expect("stdout must be JSON");
    let ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["one", "two"]);
}

#[test]
fn missing_name_exits_nonzero_and_names_the_key() {
    let root = fixture(
        r#"
[[table]]
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
"#,
    );

    let out = query(&root, "SELECT id FROM records");
    assert!(
        !out.status.success(),
        "a [[table]] without `name` must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("name") && stderr.contains("[[table]]"),
        "stderr must name the missing `name` key of the [[table]] entry, got {stderr:?}"
    );
}

#[test]
fn name_the_ddl_never_creates_exits_nonzero_before_ingestion() {
    let root = fixture(
        r#"
[[table]]
name = "messages"
ddl = "CREATE TABLE records (id TEXT)"
glob = "data/*.json"
on-file = "cat {path}"
"#,
    );

    let out = query(&root, "SELECT id FROM messages");
    assert!(
        !out.status.success(),
        "a `name` absent from the catalog must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("table 'messages'"),
        "stderr must carry the config-entry prefix `table 'messages'`, got {stderr:?}"
    );
}
