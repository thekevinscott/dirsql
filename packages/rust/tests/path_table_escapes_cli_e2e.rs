//! CLI e2e for backslash escapes in a path-table's literal text: the real
//! `dirsql` binary over a real temp directory. A `\` quotes the character
//! after it, as in bash, in the directories ahead of the first wildcard as
//! much as in the glob itself.

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    for dir in ["docs", "my dir", "x/y", "sub"] {
        fs::create_dir_all(root.path().join(dir)).unwrap();
    }
    for file in [
        "docs/api.md",
        "my dir/n.md",
        "x/y/z.md",
        ".env",
        "sub/.draft.md",
    ] {
        fs::write(root.path().join(file), "").unwrap();
    }
    root
}

/// A repo whose `.gitignore` hides `a.log`.
fn repo() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".gitignore"), "*.log\n").unwrap();
    fs::write(root.path().join("a.log"), "").unwrap();
    root
}

fn run(dir: &TempDir, table: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args([
            "query",
            &format!("SELECT path FROM '{table}'"),
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql` failed")
}

fn paths(dir: &TempDir, table: &str) -> Vec<String> {
    let out = run(dir, table);
    assert!(
        out.status.success(),
        "{table}: expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> =
        serde_json::from_slice(&out.stdout).expect("stdout must be a JSON array");
    rows.into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn an_escaped_letter_in_the_walked_directory_is_the_letter() {
    assert_eq!(paths(&fixture(), r"./do\cs/*.md"), ["docs/api.md"]);
}

#[test]
fn an_escaped_space_in_the_walked_directory_is_a_space() {
    assert_eq!(paths(&fixture(), r"./my\ dir/*.md"), ["my dir/n.md"]);
}

#[test]
fn an_escape_in_a_nested_walked_directory_is_unescaped() {
    assert_eq!(paths(&fixture(), r"./x/\y/*.md"), ["x/y/z.md"]);
}

#[test]
fn an_escape_in_the_parent_of_a_literal_file_is_unescaped() {
    assert_eq!(paths(&fixture(), r"./do\cs/api.md"), ["docs/api.md"]);
}

#[test]
fn an_escaped_directory_name_lists_the_directory() {
    assert_eq!(paths(&fixture(), r"./\do\cs"), ["docs/api.md"]);
}

#[test]
fn an_escaped_leading_dot_spells_a_dot_file() {
    assert_eq!(paths(&fixture(), r"./\.env"), [".env"]);
}

#[test]
fn an_escaped_leading_dot_below_a_wildcard_spells_a_dot_file() {
    assert_eq!(paths(&fixture(), r"./*/\.draft.md"), ["sub/.draft.md"]);
}

#[test]
fn an_escaped_spelling_of_a_gitignored_file_still_names_it() {
    assert_eq!(paths(&repo(), r"./a\.log"), ["a.log"]);
}

#[test]
fn an_escaped_spelling_in_a_brace_word_still_names_the_gitignored_file() {
    assert_eq!(paths(&repo(), r"./{a\.log,b}"), ["a.log"]);
}
