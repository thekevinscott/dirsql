//! Integration tests for backslash escapes in a path-table's literal text: a
//! `\` quotes the character after it, as in bash, in the directories the walk
//! starts from, in a spelled dot-name, and in a path that `.gitignore` would
//! otherwise hide. Real filesystem, real SQLite, SDK public API.

use std::fs;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".gitignore"), "*.log\n").unwrap();
    for dir in ["docs", "my dir", "sub"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    for file in [
        "docs/api.md",
        "my dir/n.md",
        ".env",
        "sub/.draft.md",
        "a.log",
        "{q}.log",
    ] {
        fs::write(root.path().join(file), "").unwrap();
    }
    root
}

fn paths(root: &TempDir, table: &str) -> Vec<String> {
    let db = DirSQL::builder().root(root.path()).build().unwrap();
    db.query(&format!("SELECT path FROM '{table}'"))
        .unwrap()
        .iter()
        .map(|r| match r.get("path") {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("path was not text: {other:?}"),
        })
        .collect()
}

#[test]
fn escaped_walked_directories_are_unescaped() {
    let root = fixture();
    assert_eq!(paths(&root, r"./do\cs/*.md"), ["docs/api.md"]);
    assert_eq!(paths(&root, r"./my\ dir/*.md"), ["my dir/n.md"]);
    assert_eq!(paths(&root, r"./do\cs/api.md"), ["docs/api.md"]);
}

#[test]
fn an_escaped_leading_dot_spells_a_dot_name() {
    let root = fixture();
    assert_eq!(paths(&root, r"./\.env"), [".env"]);
    assert_eq!(paths(&root, r"./*/\.draft.md"), ["sub/.draft.md"]);
}

#[test]
fn an_escaped_spelling_of_a_gitignored_file_names_it() {
    let root = fixture();
    assert_eq!(paths(&root, r"./a\.log"), ["a.log"]);
    assert_eq!(paths(&root, r"./{a\.log,b}"), ["a.log"]);
}

#[test]
fn a_literal_brace_spelling_of_a_gitignored_file_names_it() {
    let root = fixture();
    assert_eq!(paths(&root, r"./\{q\}.log"), ["{q}.log"]);
    assert_eq!(paths(&root, "./{q}.log"), ["{q}.log"]);
}
