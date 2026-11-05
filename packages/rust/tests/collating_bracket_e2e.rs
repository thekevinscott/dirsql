//! Collating symbols (`[[.a.]]`) and equivalence classes (`[[=a=]]`) inside a
//! bracket expression match as bash does, on both the path-table and the
//! `[[table]] glob` surfaces. Real binary, real files, nothing mocked.

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn fixture(glob: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    for name in ["a.json", "b.json", "c.json", "-.json"] {
        fs::write(root.path().join(name), format!(r#"{{"id": "{name}"}}"#)).unwrap();
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

fn column(root: &TempDir, sql: &str, name: &str) -> Vec<String> {
    let out = query(root, sql);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut values: Vec<String> = serde_json::from_slice::<Vec<Value>>(&out.stdout)
        .unwrap()
        .iter()
        .map(|r| r[name].as_str().unwrap().to_string())
        .collect();
    values.sort();
    values
}

fn path_matches(pattern: &str) -> Vec<String> {
    let root = fixture("a.json");
    column(&root, &format!("SELECT path FROM './{pattern}'"), "path")
}

fn table_matches(glob: &str) -> Vec<String> {
    column(&fixture(glob), "SELECT id FROM t", "id")
}

#[test]
fn a_path_table_collating_symbol_matches_its_character() {
    assert_eq!(path_matches("[[.a.]].json"), vec!["a.json"]);
}

#[test]
fn a_path_table_equivalence_class_matches_its_character() {
    assert_eq!(path_matches("[[=a=]].json"), vec!["a.json"]);
}

#[test]
fn a_path_table_named_collating_symbol_matches_its_character() {
    assert_eq!(path_matches("[[.hyphen.]].json"), vec!["-.json"]);
}

#[test]
fn a_path_table_collating_symbol_bounds_a_range() {
    assert_eq!(path_matches("[[.a.]-b].json"), vec!["a.json", "b.json"]);
}

#[test]
fn a_dash_after_an_equivalence_class_is_literal() {
    assert_eq!(
        path_matches("[[=a=]-c].json"),
        vec!["-.json", "a.json", "c.json"]
    );
}

#[test]
fn a_table_glob_collating_symbol_matches_its_character() {
    assert_eq!(table_matches("[[.b.]].json"), vec!["b.json"]);
}

#[test]
fn a_table_glob_equivalence_class_matches_its_character() {
    assert_eq!(table_matches("[[=b=]].json"), vec!["b.json"]);
}
