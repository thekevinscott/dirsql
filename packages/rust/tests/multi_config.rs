//! Integration red tests for #553: the core accepts multiple config files
//! as an ordered accumulation.
//!
//! `[[table]]` entries accumulate in list order and each config's `ignore`
//! applies to its own tables; each
//! entry's `on-file` hooks run from **its own** config file's directory; a
//! duplicate table name across entries hits the existing `DuplicateTable`
//! error. No merge step, no cross-file validation.
//!
//! Driven through the public builder's repeatable `.config()` (the #545
//! surface over #553's core plumbing) — today the second call replaces the
//! first, so every multi-entry expectation here fails on its assertions.
//!
//! Unix-only: fixtures shell out to `sh`. The Rust CI test job runs on Linux.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use dirsql::{DirSQL, DirSqlError, Value};
use tempfile::TempDir;

/// Write `.dirsql.toml` with `contents` into `dir` and return its path.
fn write_config(dir: &Path, contents: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn tables_accumulate_across_config_entries() {
    // Each config's table anchors at its own directory and matches its own file.
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    fs::write(cfg_a.path().join("a.json"), "{}").unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
name = "alpha"
ddl = "CREATE TABLE alpha (basename TEXT)"
glob = "a.json"
on-file = '''sh -c 'for p; do printf "{\"basename\":\"%s\"}\n" "${p##*/}"; done' sh'''
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    fs::write(cfg_b.path().join("b.json"), "{}").unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
name = "beta"
ddl = "CREATE TABLE beta (basename TEXT)"
glob = "b.json"
on-file = '''sh -c 'for p; do printf "{\"basename\":\"%s\"}\n" "${p##*/}"; done' sh'''
"#,
    );

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build()
        .expect("two config entries must both load");

    let alpha = db
        .query("SELECT basename FROM alpha")
        .expect("the FIRST config's table must be queryable");
    assert_eq!(alpha.len(), 1);
    assert_eq!(alpha[0]["basename"], Value::Text("a.json".into()));

    let beta = db
        .query("SELECT basename FROM beta")
        .expect("the SECOND config's table must be queryable");
    assert_eq!(beta.len(), 1);
    assert_eq!(beta[0]["basename"], Value::Text("b.json".into()));
}

#[test]
fn each_on_file_runs_from_its_declaring_config_dir() {
    // Each config's relative `on-file` script proves the hook's cwd was that config's own directory.
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    fs::write(cfg_a.path().join("a.json"), "{}").unwrap();
    fs::write(
        cfg_a.path().join("emit.sh"),
        "#!/bin/sh\nprintf '{\"v\":\"from-a\"}'\n",
    )
    .unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
name = "alpha"
ddl = "CREATE TABLE alpha (v TEXT)"
glob = "a.json"
on-file = "sh ./emit.sh"
"#,
    );

    let cfg_b = TempDir::new().unwrap();
    fs::write(cfg_b.path().join("b.json"), "{}").unwrap();
    fs::write(
        cfg_b.path().join("emit.sh"),
        "#!/bin/sh\nprintf '{\"v\":\"from-b\"}'\n",
    )
    .unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
name = "beta"
ddl = "CREATE TABLE beta (v TEXT)"
glob = "b.json"
on-file = "sh ./emit.sh"
"#,
    );

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build()
        .expect("two config entries must both load");

    let alpha = db
        .query("SELECT v FROM alpha")
        .expect("the first config's table must be queryable");
    assert_eq!(alpha[0]["v"], Value::Text("from-a".into()));

    let beta = db
        .query("SELECT v FROM beta")
        .expect("the second config's table must be queryable");
    assert_eq!(beta[0]["v"], Value::Text("from-b".into()));
}

#[test]
fn a_timeout_wrapped_hook_in_one_config_fails_the_build_under_its_table() {
    // Config A wraps ITS slow hook in
    // timeout(1): the kill fails the build, and the error names A's table
    // rather than config B's fast one.
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    fs::write(cfg_a.path().join("a.json"), "{}").unwrap();
    fs::write(
        cfg_a.path().join("slow.sh"),
        "#!/bin/sh\nsleep 3\nprintf '{\"v\":\"too-late\"}'\n",
    )
    .unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
name = "slow"
ddl = "CREATE TABLE slow (v TEXT)"
glob = "a.json"
on-file = "timeout 0.5 sh ./slow.sh"
"#,
    );

    let cfg_b = TempDir::new().unwrap();
    fs::write(cfg_b.path().join("b.json"), "{}").unwrap();
    fs::write(
        cfg_b.path().join("fast.sh"),
        "#!/bin/sh\nprintf '{\"v\":\"in-time\"}'\n",
    )
    .unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
name = "fast"
ddl = "CREATE TABLE fast (v TEXT)"
glob = "b.json"
on-file = "sh ./fast.sh"
"#,
    );

    let err = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build()
        .err()
        .expect("a hook killed by its timeout(1) wrapper fails the build");

    assert!(
        matches!(&err, DirSqlError::TableCommand { name, .. } if name == "slow"),
        "got: {err}"
    );
}

#[test]
fn ignore_patterns_apply_only_to_their_own_configs_tables() {
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    for dir in ["skip_a", "skip_b"] {
        fs::create_dir_all(cfg_a.path().join(dir)).unwrap();
        fs::write(cfg_a.path().join(dir).join("x.json"), "{}").unwrap();
    }
    fs::write(cfg_a.path().join("keep.json"), "{}").unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[dirsql]
ignore = ["skip_a/**"]

[[table]]
name = "files"
ddl = "CREATE TABLE files (basename TEXT)"
glob = "**/*.json"
on-file = '''sh -c 'for p; do printf "{\"basename\":\"%s\"}\n" "${p##*/}"; done' sh'''
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[dirsql]
ignore = ["**"]
"#,
    );

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build()
        .expect("two config entries must both load");

    let rows = db
        .query("SELECT basename FROM files ORDER BY basename")
        .expect("the first config's table must be queryable");
    assert_eq!(
        rows.len(),
        2,
        "config A's ignore applies, config B's does not, got {rows:?}"
    );
}

#[test]
fn duplicate_table_names_across_config_entries_error() {
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
name = "dup"
ddl = "CREATE TABLE dup (basename TEXT)"
glob = "*.json"
on-file = "cat"
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
name = "dup"
ddl = "CREATE TABLE dup (basename TEXT)"
glob = "*.json"
on-file = "cat"
"#,
    );

    let result = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build();

    let err = match result {
        Ok(_) => panic!("a table name defined by two config entries must error"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("dup"),
        "the error must name the duplicated table, got: {err}"
    );
}
