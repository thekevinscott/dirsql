//! CLI e2e for gitignore-by-default: the real `dirsql` binary over a real
//! temp directory with real `.gitignore` files, nothing mocked. Pins the
//! default exclusion, the `--no-ignore` escape hatch, and the hidden-file
//! divergence from fd/rg.

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// A tree with a gitignored `dist/`, a kept source file, a hidden directory,
/// and a `node_modules` for the built-in floor.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("dist")).unwrap();
    fs::create_dir_all(root.path().join(".hidden")).unwrap();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::write(root.path().join(".gitignore"), "dist/\n").unwrap();
    fs::write(root.path().join("dist/bundle.js"), "js").unwrap();
    fs::write(root.path().join("app.js"), "js").unwrap();
    fs::write(root.path().join(".hidden/inside.txt"), "txt").unwrap();
    fs::write(root.path().join("node_modules/pkg/index.js"), "js").unwrap();
    root
}

fn run(dir: &TempDir, args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql` failed")
}

fn basenames(out: &Output) -> Vec<String> {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).expect("stdout must be a JSON array");
    let mut names: Vec<String> = rows
        .into_iter()
        .map(|r| r["basename"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn a_default_scan_excludes_gitignored_files() {
    let dir = fixture();
    let out = run(&dir, &["query", "SELECT basename FROM './'"]);

    assert_eq!(
        basenames(&out),
        vec![".gitignore", "app.js", "inside.txt"],
        "dist/ is gitignored, node_modules is a built-in ignore, hidden files stay"
    );
}

#[test]
fn no_ignore_restores_gitignored_files() {
    let dir = fixture();
    let out = run(&dir, &["query", "SELECT basename FROM './'", "--no-ignore"]);

    let names = basenames(&out);
    assert!(
        names.contains(&"bundle.js".to_string()),
        "--no-ignore must disable gitignore respect, got: {names:?}"
    );
    assert!(
        names.contains(&"inside.txt".to_string()),
        "hidden files are scanned with or without --no-ignore, got: {names:?}"
    );
}

#[test]
fn no_ignore_keeps_the_built_in_ignore_floor() {
    let dir = fixture();
    let out = run(&dir, &["query", "SELECT basename FROM './'", "--no-ignore"]);

    let names = basenames(&out);
    assert!(
        !names.contains(&"index.js".to_string()),
        "--no-ignore disables gitignore only; node_modules stays skipped, got: {names:?}"
    );
}

#[test]
fn no_ignore_works_in_the_default_query_mode() {
    let dir = fixture();
    let out = run(&dir, &["SELECT basename FROM './'", "--no-ignore"]);

    let names = basenames(&out);
    assert!(
        names.contains(&"bundle.js".to_string()),
        "the bare `dirsql \"<sql>\"` form takes --no-ignore too, got: {names:?}"
    );
}
