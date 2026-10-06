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
    config_paths_with(root, glob, false)
}

fn config_paths_with(root: &Path, glob: &str, no_ignore: bool) -> Vec<String> {
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
    let db = DirSQL::builder()
        .root(root)
        .table(table)
        .no_ignore(no_ignore)
        .build()
        .unwrap();
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

fn repo(files: &[&str], gitignore: &str) -> TempDir {
    let root = tree(files);
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".gitignore"), gitignore).unwrap();
    root
}

#[test]
fn a_gitignored_file_is_not_a_row_of_a_config_table() {
    let root = repo(&["keep.md", "skip.md", "dist/out.md"], "skip.md\ndist/\n");
    let expected = vec!["keep.md"];
    assert_eq!(path_table_paths(root.path(), "**/*.md"), expected);
    assert_eq!(config_paths(root.path(), "**/*.md"), expected);
}

#[test]
fn no_ignore_lists_a_gitignored_file_in_a_config_table() {
    let root = repo(&["keep.md", "skip.md"], "skip.md\n");
    assert_eq!(
        config_paths_with(root.path(), "*.md", true),
        vec!["keep.md", "skip.md"]
    );
}
