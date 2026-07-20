//! Integration tests for path-table *glob semantics*: how the string a user
//! writes where a table name goes becomes a concrete scan. Real filesystem,
//! real SQLite, SDK public API.

use std::fs;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// A tree with a nested doc directory, a top-level file, a dotfile, and the
/// two directories a zero-config scan must not drown in.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs/nested")).unwrap();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::create_dir_all(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("skip")).unwrap();

    fs::write(root.path().join("top.md"), "top").unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo").unwrap();
    fs::write(root.path().join("docs/nested/deep.md"), "deep").unwrap();
    fs::write(root.path().join("node_modules/pkg/index.js"), "js").unwrap();
    fs::write(root.path().join(".git/config"), "cfg").unwrap();
    fs::write(root.path().join("skip/s.md"), "skipped").unwrap();
    root
}

fn open(root: &TempDir) -> DirSQL {
    DirSQL::new(root.path(), vec![]).unwrap()
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

fn paths(db: &DirSQL, sql: &str) -> Vec<String> {
    texts(&db.query(sql).unwrap(), "path")
}

#[test]
fn a_directory_path_scans_it_recursively() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs'"),
        vec!["docs/a.md", "docs/b.md", "docs/nested/deep.md"],
        "a directory is recursive by default"
    );
}

#[test]
fn a_trailing_slash_directory_path_also_scans_recursively() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/'"),
        vec!["docs/a.md", "docs/b.md", "docs/nested/deep.md"],
    );
}

#[test]
fn an_explicit_star_is_not_recursive() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './*'"),
        vec!["top.md"],
        "'./*' is the explicit non-recursive spelling: top level only"
    );
}

#[test]
fn an_explicit_star_inside_a_directory_is_not_recursive() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/*'"),
        vec!["docs/a.md", "docs/b.md"],
        "'./docs/*' must not reach docs/nested"
    );
}

#[test]
fn a_single_file_path_is_exactly_one_row() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './docs/a.md'").unwrap();

    assert_eq!(rows.len(), 1, "one file is one row: {rows:?}");
    assert_eq!(rows[0].get("path"), Some(&Value::Text("docs/a.md".into())));
}

#[test]
fn a_glob_metacharacter_path_is_used_as_written() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './docs/**/*.md'"),
        vec!["docs/a.md", "docs/b.md", "docs/nested/deep.md"],
    );
}

#[test]
fn a_recursive_scan_skips_vcs_and_dependency_directories() {
    let root = fixture();
    let db = open(&root);

    let found = paths(&db, "SELECT path FROM './'");

    assert!(
        !found.iter().any(|p| p.starts_with("node_modules/")),
        "node_modules must be skipped: {found:?}"
    );
    assert!(
        !found.iter().any(|p| p.starts_with(".git/")),
        ".git must be skipped: {found:?}"
    );
    assert!(
        found.contains(&"top.md".to_string()),
        "ordinary files must survive the skip rules: {found:?}"
    );
}

#[test]
fn naming_a_skipped_directory_explicitly_still_scans_it() {
    let root = fixture();
    let db = open(&root);

    assert_eq!(
        paths(&db, "SELECT path FROM './node_modules'"),
        vec!["node_modules/pkg/index.js"],
        "skip rules apply beneath the path you name, not to the path itself"
    );
}

#[test]
fn configured_ignore_patterns_apply_to_a_path_table() {
    let root = fixture();
    let db = DirSQL::with_ignore(root.path(), vec![], ["skip/**"]).unwrap();

    let found = paths(&db, "SELECT path FROM './'");

    assert!(
        !found.iter().any(|p| p.starts_with("skip/")),
        "a configured ignore pattern must apply to path-tables too: {found:?}"
    );
}

#[test]
fn an_absolute_path_table_resolves_and_reports_absolute_paths() {
    let root = fixture();
    let db = open(&root);

    let dir = root.path().display().to_string();
    let found = paths(&db, &format!("SELECT path FROM '{dir}/docs/*.md'"));

    assert_eq!(
        found,
        vec![format!("{dir}/docs/a.md"), format!("{dir}/docs/b.md")],
        "an absolute path-table reports absolute paths"
    );
}

#[test]
fn an_absolute_directory_path_scans_it_recursively() {
    let root = fixture();
    let db = open(&root);

    let dir = root.path().display().to_string();
    let found = paths(&db, &format!("SELECT path FROM '{dir}/docs'"));

    assert_eq!(
        found,
        vec![
            format!("{dir}/docs/a.md"),
            format!("{dir}/docs/b.md"),
            format!("{dir}/docs/nested/deep.md"),
        ],
    );
}

#[test]
fn an_absolute_single_file_path_is_exactly_one_row() {
    let root = fixture();
    let db = open(&root);

    let dir = root.path().display().to_string();
    let rows = db
        .query(&format!("SELECT path FROM '{dir}/docs/a.md'"))
        .unwrap();

    assert_eq!(rows.len(), 1, "one file is one row: {rows:?}");
    assert_eq!(
        rows[0].get("path"),
        Some(&Value::Text(format!("{dir}/docs/a.md")))
    );
}

#[test]
fn a_parent_relative_path_table_resolves_against_the_index_root() {
    let root = fixture();
    let inner = root.path().join("docs/nested");
    let db = DirSQL::new(&inner, vec![]).unwrap();

    let dir = root.path().display().to_string();
    let found = paths(&db, "SELECT path FROM '../*.md'");

    assert_eq!(
        found,
        vec![format!("{dir}/docs/a.md"), format!("{dir}/docs/b.md")],
        "'../' walks up from the index root and reports absolute paths"
    );
}

#[test]
fn an_absolute_path_table_reads_content_from_the_right_file() {
    let root = fixture();
    let db = open(&root);

    let dir = root.path().display().to_string();
    let rows = db
        .query(&format!(
            "SELECT path FROM '{dir}/docs/*.md' WHERE content = 'alpha'"
        ))
        .unwrap();

    assert_eq!(texts(&rows, "path"), vec![format!("{dir}/docs/a.md")]);
}

#[test]
fn a_missing_absolute_path_table_returns_no_rows() {
    let root = fixture();
    let db = open(&root);

    let rows = db
        .query("SELECT path FROM '/nonexistent-dirsql-dir/*.md'")
        .unwrap();

    assert!(rows.is_empty(), "expected no rows, got {rows:?}");
}
