//! End-to-end tests for the CLI's two extension surfaces: a config's own
//! `[[dirsql.extension]]` entries and the launcher-facing `--extension` flag
//! that overrides them.
//!
//! Spawns the real compiled `dirsql` binary against a real temp directory, a
//! real `.dirsql.toml` and a real loadable extension built from
//! `tests/fixtures/testext`. Nothing is mocked (real process, real filesystem,
//! real SQLite, real `dlopen`).
//!
//! `packages/rust/tests/extensions.rs` covers the same two behaviors at the
//! builder surface. They are covered again here because the CLI has a
//! decision of its own to make -- which of the two sources of extensions to
//! hand the builder -- and the builder tests cannot see it.
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixture config shells out to `cat`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

mod common;

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use common::build_fixture_extension;
use serde_json::Value;
use tempfile::TempDir;

/// What `dirsql_testext_answer()` returns once the fixture extension is loaded
/// onto the connection. An unloaded extension is a `no such function` error,
/// not a wrong answer.
const ANSWER: i64 = 42;

const SQL: &str = "SELECT dirsql_testext_answer() AS a";

/// A tempdir holding one parseable file and a `.dirsql.toml` that declares
/// `extension` (verbatim TOML, possibly empty) ahead of an ordinary table, so
/// the index has something to build either way.
fn fixture(extension: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.json"), r#"[{"name": "a"}]"#).unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            r#"{extension}
[[table]]
name = "files"
ddl = "CREATE TABLE files (name TEXT)"
glob = "*.json"
on-file = "cat {{path}}"
"#
        ),
    )
    .unwrap();
    root
}

/// A `[[dirsql.extension]]` entry naming `path`.
fn extension_entry(path: &Path) -> String {
    format!(
        "[[dirsql.extension]]\npath = \"{}\"\nentrypoint = \"sqlite3_extension_init\"\n",
        path.display(),
    )
}

/// Run `dirsql query <SQL> -c .dirsql.toml` in `root`, with `extra` appended.
fn query(root: &TempDir, extra: &[String]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("query")
        .arg(SQL)
        .arg("--config")
        .arg(root.path().join(".dirsql.toml"))
        .args(extra)
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

fn rows(out: &Output) -> Vec<Value> {
    assert!(
        out.status.success(),
        "query must succeed, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout must be JSON")
}

#[test]
fn a_config_declared_extension_loads_when_no_flag_overrides_it() {
    // The ordinary user path: no `--extension` anywhere, so the config's own
    // entries are the only source of extensions and must be honored.
    let ext = build_fixture_extension();
    let root = fixture(&extension_entry(&ext));

    let out = query(&root, &[]);

    assert_eq!(
        rows(&out)[0]["a"],
        Value::from(ANSWER),
        "the config's extension must be loaded onto the connection",
    );
}

#[test]
fn an_extension_flag_replaces_the_config_entries_rather_than_adding_to_them() {
    // The launcher path: it has already resolved the config's entries (package
    // names and all) and hands the resolved paths back through `--extension`,
    // so the config's own entries must not be loaded a second time. The config
    // here names a path that does not exist, which would fail the build if it
    // were honored -- a green run is the proof it was not.
    let ext = build_fixture_extension();
    let root = fixture("[[dirsql.extension]]\npath = \"does/not/exist.so\"\n");

    let out = query(
        &root,
        &[
            "--extension".to_string(),
            format!("{}::sqlite3_extension_init", ext.display()),
        ],
    );

    assert_eq!(
        rows(&out)[0]["a"],
        Value::from(ANSWER),
        "the flag's extension must load and the config's must be suppressed",
    );
}
