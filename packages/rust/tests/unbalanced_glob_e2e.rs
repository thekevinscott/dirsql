//! A `{`, `}` or `[` with no partner is a literal character, as in bash, on
//! both the path-table and the `[[table]] glob` surfaces. Real binary, real
//! files, nothing mocked.

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

const NAMES: [&str; 5] = ["{a.json", "a}.json", "[a.json", "a[b.json", "a.json"];

fn fixture(glob: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    for name in NAMES {
        fs::write(root.path().join(name), r#"{"id": "row"}"#).unwrap();
    }
    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (id TEXT)\"\nglob = \"{glob}\"\non-file = \"cat\"\n"
        ),
    )
    .unwrap();
    root
}

fn query(root: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .arg("query")
        .arg(sql)
        .arg("--config")
        .arg(root.path().join(".dirsql.toml"))
        .current_dir(root.path())
        .output()
        .unwrap()
}

fn count(root: &TempDir, sql: &str) -> usize {
    let out = query(root, sql);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Vec<Value>>(&out.stdout)
        .unwrap()
        .len()
}

fn table_matches(glob: &str) -> usize {
    count(&fixture(glob), "SELECT id FROM t")
}

fn path_matches(pattern: &str) -> Vec<String> {
    let root = fixture("a.json");
    let out = query(&root, &format!("SELECT path FROM './{pattern}'"));
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Vec<Value>>(&out.stdout)
        .unwrap()
        .iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_table_glob_with_an_unclosed_brace_is_literal() {
    assert_eq!(table_matches("{a.json"), 1);
}

#[test]
fn a_table_glob_with_an_unopened_brace_is_literal() {
    assert_eq!(table_matches("a}.json"), 1);
}

#[test]
fn a_table_glob_with_an_unclosed_bracket_is_literal() {
    assert_eq!(table_matches("[a.json"), 1);
    assert_eq!(table_matches("a[b.json"), 1);
}

#[test]
fn a_table_glob_with_a_balanced_alternation_still_alternates() {
    assert_eq!(table_matches("{a,b}.json"), 1);
}

#[test]
fn a_path_table_with_an_unclosed_bracket_is_literal() {
    assert_eq!(path_matches("[a.json"), vec!["[a.json"]);
    assert_eq!(path_matches("a[b.json"), vec!["a[b.json"]);
}

#[test]
fn a_path_table_with_an_unclosed_bracket_before_a_brace_group_is_literal() {
    assert!(path_matches("[{q}.json").is_empty());
}

#[test]
fn a_path_table_with_unbalanced_braces_is_literal() {
    assert_eq!(path_matches("{a.json"), vec!["{a.json"]);
    assert_eq!(path_matches("a}.json"), vec!["a}.json"]);
}
