//! Integration red tests for #553: the core accepts multiple config files
//! as an ordered accumulation.
//!
//! `[[table]]` and `ignore` accumulate across entries in list order; each
//! entry's `on-file` hooks run from **its own** config file's directory under
//! **its own** file's `hook-timeout`; a duplicate table name across entries
//! hits the existing `DuplicateTable` error. No merge step, no cross-file
//! validation.
//!
//! Driven through the public builder's repeatable `.config()` (the #545
//! surface over #553's core plumbing) — today the second call replaces the
//! first, so every multi-entry expectation here fails on its assertions.
//!
//! Unix-only: fixtures shell out to `sh`. The Rust CI test job runs on Linux.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// Write `.dirsql.toml` with `contents` into `dir` and return its path.
fn write_config(dir: &Path, contents: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(&path, contents).unwrap();
    path
}

#[test]
fn tables_accumulate_across_config_entries() {
    // Distinct globs per table: dirsql routes each file to a single table
    // (one-file-one-table), so each config's table matches its own file.
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();
    fs::write(data.path().join("b.json"), "{}").unwrap();

    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
ddl = "CREATE TABLE alpha (basename TEXT)"
glob = "a.json"
on-file = '''sh -c 'printf "[{\"basename\":\"%s\"}]" "${1##*/}"' sh {path}'''
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
ddl = "CREATE TABLE beta (basename TEXT)"
glob = "b.json"
on-file = '''sh -c 'printf "[{\"basename\":\"%s\"}]" "${1##*/}"' sh {path}'''
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
    // Distinct globs (one-file-one-table); each config's relative `on-file`
    // script proves the hook's cwd was that config's own directory.
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();
    fs::write(data.path().join("b.json"), "{}").unwrap();

    let cfg_a = TempDir::new().unwrap();
    fs::write(
        cfg_a.path().join("emit.sh"),
        "#!/bin/sh\nprintf '[{\"v\":\"from-a\"}]'\n",
    )
    .unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
ddl = "CREATE TABLE alpha (v TEXT)"
glob = "a.json"
on-file = "sh ./emit.sh {path}"
"#,
    );

    let cfg_b = TempDir::new().unwrap();
    fs::write(
        cfg_b.path().join("emit.sh"),
        "#!/bin/sh\nprintf '[{\"v\":\"from-b\"}]'\n",
    )
    .unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
ddl = "CREATE TABLE beta (v TEXT)"
glob = "b.json"
on-file = "sh ./emit.sh {path}"
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
fn hook_timeout_scopes_to_its_declaring_config() {
    // Distinct globs (one-file-one-table). Config A bounds ITS hooks at 1s and
    // declares a 3s hook: its rows are skipped. Config B declares no timeout:
    // its fast hook is unaffected.
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();
    fs::write(data.path().join("b.json"), "{}").unwrap();

    let cfg_a = TempDir::new().unwrap();
    fs::write(
        cfg_a.path().join("slow.sh"),
        "#!/bin/sh\nsleep 3\nprintf '[{\"v\":\"too-late\"}]'\n",
    )
    .unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[dirsql]
hook-timeout = 1

[[table]]
ddl = "CREATE TABLE slow (v TEXT)"
glob = "a.json"
on-file = "sh ./slow.sh {path}"
"#,
    );

    let cfg_b = TempDir::new().unwrap();
    fs::write(
        cfg_b.path().join("fast.sh"),
        "#!/bin/sh\nprintf '[{\"v\":\"in-time\"}]'\n",
    )
    .unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
ddl = "CREATE TABLE fast (v TEXT)"
glob = "b.json"
on-file = "sh ./fast.sh {path}"
"#,
    );

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .build()
        .expect("two config entries must both load");

    let slow = db
        .query("SELECT v FROM slow")
        .expect("the timed-out table must still exist");
    assert_eq!(
        slow.len(),
        0,
        "a hook exceeding its own file's timeout skips rows"
    );

    let fast = db
        .query("SELECT v FROM fast")
        .expect("the second config's table must be queryable");
    assert_eq!(fast[0]["v"], Value::Text("in-time".into()));
}

#[test]
fn ignore_patterns_accumulate_across_config_entries() {
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("keep.json"), "{}").unwrap();
    fs::create_dir_all(data.path().join("skip_a")).unwrap();
    fs::write(data.path().join("skip_a").join("x.json"), "{}").unwrap();
    fs::create_dir_all(data.path().join("skip_b")).unwrap();
    fs::write(data.path().join("skip_b").join("y.json"), "{}").unwrap();

    // The table lives in config A; config B contributes only an ignore
    // pattern — which must still apply to A's table (global accumulation).
    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[dirsql]
ignore = ["**/skip_a/**"]

[[table]]
ddl = "CREATE TABLE files (basename TEXT)"
glob = "**/*.json"
on-file = '''sh -c 'printf "[{\"basename\":\"%s\"}]" "${1##*/}"' sh {path}'''
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[dirsql]
ignore = ["**/skip_b/**"]
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
        1,
        "both configs' ignore patterns must apply, got {rows:?}"
    );
    assert_eq!(rows[0]["basename"], Value::Text("keep.json".into()));
}

#[test]
fn duplicate_table_names_across_config_entries_error() {
    let data = TempDir::new().unwrap();

    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_config(
        cfg_a.path(),
        r#"
[[table]]
ddl = "CREATE TABLE dup (basename TEXT)"
glob = "*.json"
"#,
    );
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(
        cfg_b.path(),
        r#"
[[table]]
ddl = "CREATE TABLE dup (basename TEXT)"
glob = "*.json"
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
