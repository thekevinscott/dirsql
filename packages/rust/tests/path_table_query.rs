//! Integration tests for path-tables in the core `query()` path: a table name
//! SQLite does not know, but which looks like a path, resolves to a live glob
//! scan of the index root. Real filesystem, real SQLite, SDK public API.

use std::collections::HashMap;
use std::fs;

use dirsql::{DirSQL, Row, Table, Value};
use tempfile::TempDir;

/// A tree with two markdown docs, one CSV, and a nested note.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::create_dir_all(root.path().join("notes/deep")).unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo body").unwrap();
    fs::write(root.path().join("docs/c.csv"), "x,y").unwrap();
    fs::write(root.path().join("notes/deep/d.md"), "delta").unwrap();
    root
}

/// A real, named dirsql table over the CSV, so path-tables can be joined
/// against ordinary tables.
fn csv_table() -> Table {
    Table::new(
        "CREATE TABLE rows_csv (path TEXT, cells TEXT)",
        "docs/*.csv",
        |path| {
            let content = fs::read_to_string(path).unwrap();
            vec![HashMap::from([
                ("path".into(), Value::Text("docs/c.csv".into())),
                ("cells".into(), Value::Text(content.trim().to_string())),
            ])]
        },
    )
}

fn open(root: &TempDir) -> DirSQL {
    DirSQL::new(root.path(), vec![csv_table()]).unwrap()
}

fn texts(rows: &[Row], column: &str) -> Vec<String> {
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| match r.get(column) {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("{column} was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn bare_dot_slash_scans_the_index_root_recursively() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './'").unwrap();

    assert_eq!(
        texts(&rows, "path"),
        vec!["docs/a.md", "docs/b.md", "docs/c.csv", "notes/deep/d.md"],
        "'./' must scan the whole index root"
    );
}

#[test]
fn a_scoped_glob_limits_the_scan() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './docs/*.md'").unwrap();

    assert_eq!(
        texts(&rows, "path"),
        vec!["docs/a.md", "docs/b.md"],
        "'./docs/*.md' must not reach nested notes or the CSV"
    );
}

#[test]
fn stat_columns_are_available_on_a_path_table() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query("SELECT basename, size FROM './docs/*.md' ORDER BY size DESC")
        .unwrap();

    assert_eq!(texts(&rows, "basename"), vec!["a.md", "b.md"]);
    assert_eq!(
        rows[0].get("size"),
        Some(&Value::Integer(10)),
        "largest file first: 'bravo body' is 10 bytes"
    );
}

#[test]
fn a_zero_match_path_table_returns_no_rows() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './docs/*.rst'").unwrap();

    assert!(rows.is_empty(), "expected no rows, got {rows:?}");
}

#[test]
fn several_path_tables_resolve_in_one_statement() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query(
            "SELECT a.path AS md, b.path AS csv \
             FROM './docs/*.md' AS a, './docs/*.csv' AS b \
             WHERE a.basename = 'a.md'",
        )
        .unwrap();

    assert_eq!(rows.len(), 1, "one md × one csv: {rows:?}");
    assert_eq!(rows[0].get("md"), Some(&Value::Text("docs/a.md".into())));
    assert_eq!(rows[0].get("csv"), Some(&Value::Text("docs/c.csv".into())));
}

#[test]
fn a_path_table_joins_against_a_named_table() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query(
            "SELECT p.basename, r.cells \
             FROM './docs/*.csv' AS p JOIN rows_csv AS r ON r.path = p.path",
        )
        .unwrap();

    assert_eq!(texts(&rows, "basename"), vec!["c.csv"]);
    assert_eq!(rows[0].get("cells"), Some(&Value::Text("x,y".into())));
}

#[test]
fn a_path_table_resolves_again_on_a_second_query() {
    let root = fixture();
    let db = open(&root);

    let first = db.query("SELECT path FROM './docs/*.md'").unwrap();
    let second = db.query("SELECT path FROM './docs/*.md'").unwrap();

    assert_eq!(texts(&first, "path"), texts(&second, "path"));
}

#[test]
fn a_path_table_reflects_files_written_after_the_index_was_built() {
    let root = fixture();
    let db = open(&root);

    fs::write(root.path().join("docs/e.md"), "echo").unwrap();
    let rows = db.query("SELECT path FROM './docs/*.md'").unwrap();

    assert!(
        texts(&rows, "path").contains(&"docs/e.md".to_string()),
        "path-table reads are live: {rows:?}"
    );
}

#[test]
fn a_double_star_glob_reaches_any_depth() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './**/*.md'").unwrap();

    assert_eq!(
        texts(&rows, "path"),
        vec!["docs/a.md", "docs/b.md", "notes/deep/d.md"]
    );
}

#[test]
fn content_is_hidden_from_star_but_filterable_by_name() {
    let root = fixture();
    let db = open(&root);

    let starred = db.query("SELECT * FROM './docs/*.md'").unwrap();
    assert!(
        !starred[0].contains_key("content"),
        "content must stay out of SELECT *: {starred:?}"
    );

    let rows = db
        .query("SELECT path FROM './docs/*.md' WHERE content LIKE '%bravo%'")
        .unwrap();
    assert_eq!(texts(&rows, "path"), vec!["docs/b.md"]);
}

#[test]
fn the_documented_stat_columns_are_all_present() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query("SELECT path, basename, dir, ext, size, mtime, ctime FROM './docs/*.csv'")
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("dir"), Some(&Value::Text("docs".into())));
    assert_eq!(rows[0].get("ext"), Some(&Value::Text("csv".into())));
    assert!(matches!(rows[0].get("mtime"), Some(Value::Integer(_))));
    assert!(matches!(rows[0].get("ctime"), Some(Value::Integer(_))));
}

#[test]
fn a_named_table_is_never_shadowed_by_the_fallback() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT cells FROM rows_csv").unwrap();

    assert_eq!(
        texts(&rows, "cells"),
        vec!["x,y"],
        "the real table must resolve, not a path scan"
    );
}

#[test]
fn a_plain_unknown_table_fails_with_the_sqlite_error_and_no_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db.query("SELECT * FROM usrs").unwrap_err().to_string();

    assert!(
        err.contains("no such table: usrs"),
        "typos must fail unchanged, got: {err}"
    );
    assert!(
        !err.contains("did you mean"),
        "a plain typo must carry no path-table hint, got: {err}"
    );
}

#[test]
fn a_bare_glob_fails_with_a_hint_naming_the_dot_slash_form() {
    let root = fixture();
    let db = open(&root);

    let err = db.query("SELECT * FROM '**/*.md'").unwrap_err().to_string();

    assert!(
        err.contains("did you mean './**/*.md'?"),
        "a bare glob must be rejected with the hint, got: {err}"
    );
}

#[test]
fn a_bare_single_character_glob_also_gets_the_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db
        .query("SELECT * FROM 'docs/?.md'")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("did you mean './docs/?.md'?"),
        "'?' is a glob metacharacter too, got: {err}"
    );
}

#[test]
fn a_bare_bracket_glob_also_gets_the_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db
        .query("SELECT * FROM 'docs/[ab].md'")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains("did you mean './docs/[ab].md'?"),
        "'[' is a glob metacharacter too, got: {err}"
    );
}

#[test]
fn a_statement_naming_several_unknown_tables_terminates() {
    let root = fixture();
    let db = open(&root);

    // The retry loop must give up rather than spin on names it cannot
    // resolve; a hang here is the failure this pins.
    let err = db
        .query("SELECT * FROM usrs JOIN also_missing")
        .unwrap_err()
        .to_string();

    assert!(err.contains("no such table"), "got: {err}");
}

#[test]
fn path_tables_never_leak_into_the_persisted_schema() {
    let root = fixture();
    let db = open(&root);

    db.query("SELECT path FROM './docs/*.md'").unwrap();
    let rows = db
        .query("SELECT name FROM main.sqlite_master WHERE type = 'table'")
        .unwrap();

    let names = texts(&rows, "name");
    assert!(
        names.contains(&"rows_csv".to_string()),
        "the real table must still be there: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.starts_with("./")),
        "a path-table lives in temp and must not appear in main's schema: {names:?}"
    );
}

#[test]
fn an_unquoted_path_fails_with_a_quoting_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db.query("SELECT * FROM ./").unwrap_err().to_string();

    assert!(
        err.contains("syntax error"),
        "SQLite's own error must survive, got: {err}"
    );
    assert!(
        err.contains(r#"did you mean "./"?"#),
        "an unquoted path must name its quoted form, got: {err}"
    );
}

#[test]
fn an_unquoted_nested_path_names_the_whole_path_in_the_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db
        .query("SELECT * FROM ./docs/a.md")
        .unwrap_err()
        .to_string();

    assert!(
        err.contains(r#"did you mean "./docs/a.md"?"#),
        "the hint must quote the whole path, not the token SQLite choked on, got: {err}"
    );
}

#[test]
fn an_ordinary_syntax_error_carries_no_quoting_hint() {
    let root = fixture();
    let db = open(&root);

    let err = db.query("SELECT * FROM").unwrap_err().to_string();

    assert!(
        !err.contains("did you mean"),
        "a syntax error with no path in it must stay unhinted, got: {err}"
    );
}
