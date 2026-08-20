//! Integration tests for the declared `[[table]] name` key.
//!
//! A table's name is *declared*, never derived: dirsql does not tokenize
//! `ddl` to learn it. A `[[table]]` entry without `name` fails to load, and a
//! `name` that the entry's `ddl` does not actually create fails at load time
//! -- checked against SQLite's own catalog, before any file is ingested.

use dirsql::{DirSQL, Value};
use std::fs;
use tempfile::TempDir;

/// An `on-file` hook emitting the file's root-relative `path`.
const HOOK: &str = r#"on-file = '''sh -c 'rel=${1#"$2"/}; printf "[{\"path\":\"%s\"}]" "$rel"' sh {path} {root}'''"#;

/// A tempdir with one matched file and a `.dirsql.toml` holding `config`.
fn fixture(config: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/a.csv"), "anything").unwrap();
    fs::write(root.path().join(".dirsql.toml"), config).unwrap();
    root
}

fn build(root: &TempDir) -> dirsql::Result<DirSQL> {
    DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
}

#[test]
fn declared_name_registers_the_table() {
    let root = fixture(&format!(
        r#"
[[table]]
name = "notes"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
{HOOK}
"#
    ));

    let db = build(&root).expect("a [[table]] with a matching `name` must load");
    let rows = db.query("SELECT path FROM notes").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("data/a.csv".into()));
}

#[test]
fn missing_name_is_a_load_error() {
    let root = fixture(&format!(
        r#"
[[table]]
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
{HOOK}
"#
    ));

    let message = match build(&root) {
        Ok(_) => panic!("a [[table]] without `name` must fail to load"),
        Err(err) => err.to_string(),
    };
    assert!(
        message.contains("name") && message.contains("[[table]]"),
        "the error must name the missing `name` key of the [[table]] entry, got {message:?}"
    );
}

#[test]
fn name_the_ddl_never_creates_is_a_load_error() {
    let root = fixture(&format!(
        r#"
[[table]]
name = "messages"
ddl = "CREATE TABLE notes (path TEXT)"
glob = "data/*.csv"
{HOOK}
"#
    ));

    let message = match build(&root) {
        Ok(_) => panic!("a `name` absent from the catalog must fail to load"),
        Err(err) => err.to_string(),
    };
    assert!(
        message.contains("table 'messages'"),
        "the error must carry the config-entry prefix `table 'messages'`, got {message:?}"
    );
}

/// A quoted or schema-qualified identifier is a SQL *delimiter* form, not part
/// of the name: SQLite records the bare name in its catalog, so the declared
/// `name` matches without dirsql interpreting the DDL text at all.
#[test]
fn quoted_and_schema_qualified_ddl_match_the_declared_name() {
    for ddl in [
        r#"CREATE TABLE \"notes\" (path TEXT)"#,
        "CREATE TABLE main.notes (path TEXT)",
        "CREATE TABLE IF NOT EXISTS notes (path TEXT)",
    ] {
        let root = fixture(&format!(
            r#"
[[table]]
name = "notes"
ddl = "{ddl}"
glob = "data/*.csv"
{HOOK}
"#
        ));

        let db = build(&root).unwrap_or_else(|e| panic!("{ddl} must load, got {e}"));
        let rows = db.query("SELECT path FROM notes").unwrap();
        assert_eq!(rows.len(), 1, "{ddl} must index the matched file");
    }
}
