//! A config `[[table]] glob` reads exactly as the same string written after
//! `FROM './`: both surfaces run one pattern implementation. Real filesystem,
//! real SQLite, SDK public API.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use dirsql::{DirSQL, Table, Value};
use tempfile::TempDir;

fn tree(files: &[&str]) -> TempDir {
    let root = TempDir::new().unwrap();
    for file in files {
        let at = root.path().join(file);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, "x").unwrap();
    }
    root
}

fn config_paths(root: &Path, glob: &str) -> Vec<String> {
    let base = root.to_path_buf();
    let table = Table::new("t", "CREATE TABLE t (p TEXT)", glob, move |path| {
        let rel = Path::new(path)
            .strip_prefix(&base)
            .unwrap_or(Path::new(path));
        vec![HashMap::from([(
            "p".to_string(),
            Value::Text(rel.to_string_lossy().replace('\\', "/")),
        )])]
    });
    let db = DirSQL::new(root, vec![table]).unwrap();
    texts(&db, "SELECT p FROM t", "p")
}

fn path_table_paths(root: &Path, glob: &str) -> Vec<String> {
    let db = DirSQL::new(root, vec![]).unwrap();
    texts(&db, &format!("SELECT path FROM './{glob}'"), "path")
}

fn texts(db: &DirSQL, sql: &str, column: &str) -> Vec<String> {
    let mut out: Vec<String> = db
        .query(sql)
        .unwrap()
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
fn a_brace_range_expands_as_it_does_in_a_path_table() {
    let root = tree(&["f1.md", "f2.md", "f3.md", "f4.md"]);
    let expected = vec!["f1.md", "f2.md", "f3.md"];
    assert_eq!(path_table_paths(root.path(), "f{1..3}.md"), expected);
    assert_eq!(config_paths(root.path(), "f{1..3}.md"), expected);
}

#[test]
fn an_unbalanced_brace_or_bracket_is_literal_as_it_is_in_a_path_table() {
    let root = tree(&["{a.md", "a}.md", "[a.md", "a.md"]);
    for (glob, expected) in [("{a.md", "{a.md"), ("a}.md", "a}.md"), ("[a.md", "[a.md")] {
        assert_eq!(path_table_paths(root.path(), glob), vec![expected]);
        assert_eq!(config_paths(root.path(), glob), vec![expected]);
    }
}
