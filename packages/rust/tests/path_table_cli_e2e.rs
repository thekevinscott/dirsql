//! CLI e2e for path-tables: the real `dirsql` binary, a real temp directory,
//! nothing mocked. Pins the surface a user actually types.

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo body").unwrap();
    fs::write(root.path().join("docs/c.csv"), "x,y").unwrap();
    root
}

fn run(dir: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

/// Run with `home` as the process home directory, so `~/` resolves somewhere
/// the test owns rather than the developer's real home.
fn run_with_home(dir: &TempDir, home: &TempDir, sql: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .current_dir(dir.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .expect("spawning `dirsql query` failed")
}

fn rows(out: &Output) -> Vec<Value> {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout must be a JSON array")
}

fn basenames(out: &Output) -> Vec<String> {
    let mut names: Vec<String> = rows(out)
        .into_iter()
        .map(|r| r["basename"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn bare_dot_slash_returns_stat_rows_for_the_working_directory() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './'");

    assert_eq!(basenames(&out), vec!["a.md", "b.md", "c.csv"]);
}

#[test]
fn a_scoped_glob_limits_the_cli_scan() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/*.md'");

    assert_eq!(basenames(&out), vec!["a.md", "b.md"]);
}

#[test]
fn two_path_tables_join_against_each_other() {
    let dir = fixture();
    let out = run(
        &dir,
        "SELECT p.basename FROM './docs/*.md' AS p \
         JOIN './' AS f ON f.path = p.path",
    );

    assert_eq!(basenames(&out), vec!["a.md", "b.md"]);
}

#[test]
fn a_zero_match_path_table_prints_an_empty_array() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/*.rst'");

    assert_eq!(rows(&out), Vec::<Value>::new());
}

#[test]
fn a_bare_glob_fails_with_the_dot_slash_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM '**/*.md'");

    assert!(!out.status.success(), "a bare glob must not succeed");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("did you mean './**/*.md'?"),
        "expected the hint, got: {stderr}"
    );
}

#[test]
fn a_recursive_scan_omits_ignored_directories() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join("node_modules/pkg/index.js"), "js").unwrap();
    fs::write(dir.path().join(".git/config"), "cfg").unwrap();

    let out = run(&dir, "SELECT path FROM './'");
    let found: Vec<String> = rows(&out)
        .into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect();

    assert!(
        !found.iter().any(|p| p.starts_with("node_modules/")),
        "node_modules must not drown the scan: {found:?}"
    );
    assert!(
        !found.iter().any(|p| p.starts_with(".git/")),
        ".git must not drown the scan: {found:?}"
    );
    assert!(
        found.contains(&"docs/a.md".to_string()),
        "ordinary files must survive: {found:?}"
    );
}

#[test]
fn an_explicit_star_returns_only_top_level_files() {
    let dir = fixture();
    fs::write(dir.path().join("top.md"), "top").unwrap();

    let out = run(&dir, "SELECT basename FROM './*'");

    assert_eq!(
        basenames(&out),
        vec!["top.md"],
        "'./*' must not descend into docs/"
    );
}

#[test]
fn a_directory_path_is_recursive_by_default() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs'");

    assert_eq!(basenames(&out), vec!["a.md", "b.md", "c.csv"]);
}

#[test]
fn a_single_file_path_returns_exactly_one_row() {
    let dir = fixture();
    let out = run(&dir, "SELECT basename FROM './docs/a.md'");

    assert_eq!(basenames(&out), vec!["a.md"]);
}

#[test]
fn a_home_relative_path_table_resolves_against_the_home_directory() {
    let dir = fixture();
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join("notes")).unwrap();
    fs::write(home.path().join("notes/n.md"), "note").unwrap();

    let out = run_with_home(&dir, &home, "SELECT path, basename FROM '~/notes/*.md'");

    assert_eq!(basenames(&out), vec!["n.md"]);
    let path = rows(&out)[0]["path"].as_str().unwrap().to_string();
    assert_eq!(
        path,
        home.path().join("notes/n.md").display().to_string(),
        "a '~/' path-table reports absolute paths"
    );
}

#[test]
fn an_absolute_path_table_resolves_outside_the_index_root() {
    let dir = fixture();
    let other = TempDir::new().unwrap();
    fs::write(other.path().join("o.md"), "other").unwrap();

    let out = run(
        &dir,
        &format!("SELECT path FROM '{}/*.md'", other.path().display()),
    );
    let found: Vec<String> = rows(&out)
        .into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect();

    assert_eq!(found, vec![other.path().join("o.md").display().to_string()]);
}

#[test]
fn a_typoed_table_name_fails_without_a_hint() {
    let dir = fixture();
    let out = run(&dir, "SELECT * FROM usrs");

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no such table: usrs"),
        "expected the plain SQLite error, got: {stderr}"
    );
    assert!(
        !stderr.contains("did you mean"),
        "a typo must carry no path-table hint, got: {stderr}"
    );
}
