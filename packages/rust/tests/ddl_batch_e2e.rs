//! End-to-end tests for `ddl` as a multi-statement SQL batch.
//!
//! Spawns the real compiled `dirsql` binary against a real temp directory and
//! a real `.dirsql.toml`. Nothing is mocked (real process, real filesystem,
//! real SQLite, real `on-file` command spawn).
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixture shells out to `cat`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

mod common;

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use common::build_fixture_extension;
use serde_json::Value;
use tempfile::TempDir;

/// A tempdir holding one JSON file of rows and a `.dirsql.toml` with `config`.
fn fixture(config: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(
        root.path().join("data/a.json"),
        r#"[{"id": "one", "body": "hello world"}, {"id": "two", "body": "goodbye moon"}]"#,
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

/// Like [`query`], but keeps the on-disk cache across runs (`--persist`), so a
/// second invocation reuses — or, on an edited `ddl`, sweeps — the first's.
fn query_persist(root: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("query")
        .arg(sql)
        .arg("--config")
        .arg(root.path().join(".dirsql.toml"))
        .arg("--persist")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql query --persist` failed")
}

fn rows(out: &Output) -> Vec<Value> {
    assert!(
        out.status.success(),
        "query must succeed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout must be JSON")
}

/// A batch declaring the row table, a B-tree index over it, an FTS5 index and
/// the trigger that fills it — every statement in one `ddl` key.
const BATCH_CONFIG: &str = r#"
[[table]]
name = "records"
glob = "data/*.json"
on-file = "cat {path}"
ddl = '''
CREATE TABLE records (id TEXT, body TEXT);
CREATE INDEX records_id ON records(id);
CREATE VIRTUAL TABLE records_fts USING fts5(body, content='records', content_rowid='rowid');
CREATE TRIGGER records_ai AFTER INSERT ON records BEGIN
  INSERT INTO records_fts(rowid, body) VALUES (new.rowid, new.body);
END;
'''
"#;

#[test]
fn a_multi_statement_batch_indexes_and_queries_through_the_cli() {
    let root = fixture(BATCH_CONFIG);

    let out = query(&root, "SELECT id FROM records ORDER BY id");
    let parsed = rows(&out);
    let ids: Vec<&str> = parsed.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["one", "two"]);

    let out = query(&root, "SELECT name FROM pragma_index_list('records')");
    let parsed = rows(&out);
    let names: Vec<String> = parsed
        .iter()
        .map(|r| r["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == "records_id"),
        "the batch's CREATE INDEX must have run, got {names:?}"
    );
}

#[test]
fn keyword_search_over_an_fts5_index_declared_in_ddl() {
    let root = fixture(BATCH_CONFIG);

    let out = query(
        &root,
        "SELECT body FROM records_fts WHERE records_fts MATCH 'hello'",
    );
    let parsed = rows(&out);
    let bodies: Vec<&str> = parsed.iter().map(|r| r["body"].as_str().unwrap()).collect();
    assert_eq!(
        bodies,
        ["hello world"],
        "the insert trigger must have filled the FTS5 index"
    );
}

#[test]
fn a_batch_sqlite_rejects_exits_nonzero_naming_the_table() {
    let root = fixture(
        r#"
[[table]]
name = "records"
glob = "data/*.json"
on-file = "cat {path}"
ddl = '''
CREATE TABLE records (id TEXT, body TEXT);
CREATE TABLE oops (
'''
"#,
    );

    let out = query(&root, "SELECT id FROM records");
    assert!(
        !out.status.success(),
        "a batch SQLite rejects must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("table 'records'") && stderr.contains("incomplete input"),
        "stderr must prefix SQLite's raw error with the config entry, got {stderr:?}"
    );
}

#[test]
fn a_declared_name_that_is_a_virtual_table_exits_nonzero() {
    let root = fixture(
        r#"
[[table]]
name = "records"
glob = "data/*.json"
on-file = "cat {path}"
ddl = "CREATE VIRTUAL TABLE records USING fts5(id, body)"
"#,
    );

    let out = query(&root, "SELECT id FROM records");
    assert!(
        !out.status.success(),
        "a declared table that is virtual must exit non-zero"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("table 'records'") && stderr.contains("virtual"),
        "stderr must name the entry and say the table is virtual, got {stderr:?}"
    );
}

/// Editing the `ddl` of a `--persist` cache that holds a virtual table from a
/// loaded extension must rebuild it, not wedge it. The sweep that precedes the
/// rebuild issues `DROP TABLE records_ext`, which only a connection carrying
/// the extension's module can execute — and once it fails, every later run
/// repeats the failure until the cache is deleted by hand.
#[test]
fn a_persisted_cache_holding_an_extension_virtual_table_survives_a_ddl_edit() {
    let ext = build_fixture_extension();
    let config = |extra: &str| {
        format!(
            r#"
[[dirsql.extension]]
path = "{}"
entrypoint = "sqlite3_extension_init"

[[table]]
name = "records"
glob = "data/*.json"
on-file = "cat {{path}}"
ddl = '''
CREATE TABLE records (id TEXT, body TEXT);
CREATE VIRTUAL TABLE records_ext USING dirsql_testext_vtab();{extra}
'''
"#,
            ext.display(),
        )
    };

    let root = fixture(&config(""));
    let first = query_persist(&root, "SELECT n FROM records_ext");
    assert_eq!(
        rows(&first)[0]["n"],
        Value::from(42),
        "the extension's virtual table must be queryable on a cold build"
    );

    fs::write(
        root.path().join(".dirsql.toml"),
        config("\nCREATE INDEX records_id ON records(id);"),
    )
    .unwrap();

    let second = query_persist(&root, "SELECT n FROM records_ext");
    assert!(
        second.status.success(),
        "an edited ddl must sweep and rebuild the cache, stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(rows(&second)[0]["n"], Value::from(42));

    // The wedge the issue describes is permanent: assert the *next* run is
    // clean too, not merely that one rebuild squeaked through.
    let third = query_persist(&root, "SELECT n FROM records_ext");
    assert!(
        third.status.success(),
        "the rebuilt cache must stay openable, stderr: {}",
        String::from_utf8_lossy(&third.stderr)
    );
}
