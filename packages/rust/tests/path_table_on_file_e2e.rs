//! CLI e2e for the `--on-file` flag: the real `dirsql` binary, a real temp
//! directory, a real parser script, nothing mocked. Pins the surface a user
//! actually types: `dirsql query "<sql>" --on-file '<command>'`.

#![cfg(feature = "cli")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// A real parser: reads the file `$1`, fails loudly on a poisoned file, and
/// otherwise prints a one-line JSON array of row objects derived from the
/// content (the on-file contract's payload is the last non-empty stdout line).
const PARSER_SCRIPT: &str = r#"#!/bin/sh
f="$1"
if grep -q POISON "$f"; then
  echo "poison detected in $f" >&2
  exit 1
fi
title=$(head -n1 "$f")
words=$(wc -w < "$f" | tr -d ' ')
printf '[{"title":"%s","words":%s}]\n' "$title" "$words"
"#;

/// A temp tree of markdown-like files plus an executable `parse.sh` at the
/// root. The parser is invoked as `./parse.sh {path}`, resolved against the
/// scan root (the invocation cwd for a `./` path-table).
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/a.md"), "alpha title\nbody one two").unwrap();
    fs::write(root.path().join("docs/b.md"), "bravo title\nbody three").unwrap();

    let script = root.path().join("parse.sh");
    fs::write(&script, PARSER_SCRIPT).unwrap();
    let mut perms = fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script, perms).unwrap();
    root
}

fn run_on_file(dir: &TempDir, sql: &str, parser: &str) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .arg("--on-file")
        .arg(parser)
        .current_dir(dir.path())
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

fn titles(out: &Output) -> Vec<String> {
    let mut names: Vec<String> = rows(out)
        .into_iter()
        .map(|r| r["title"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[test]
fn a_parser_supplies_the_rows_and_the_schema() {
    let dir = fixture();
    let out = run_on_file(&dir, "SELECT title, words FROM './**/*.md'", "./parse.sh {path}");

    assert_eq!(titles(&out), vec!["alpha title", "bravo title"]);
    // The schema is the parser's output: `words` came from the parser, not a
    // stat column.
    let first = &rows(&out)[0];
    assert!(first.get("words").is_some(), "parser column present: {first:?}");
}

#[test]
fn stat_columns_are_not_reachable_on_a_parsed_table() {
    let dir = fixture();
    // `size` is a stat column; a parsed path-table's schema is the parser's
    // output alone, so it is gone.
    let out = run_on_file(&dir, "SELECT size FROM './**/*.md'", "./parse.sh {path}");

    assert!(
        !out.status.success(),
        "a stat column must not resolve on a parsed table"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no such column") || stderr.contains("size"),
        "expected a missing-column error, got: {stderr}"
    );
}

#[test]
fn a_failing_file_is_skipped_with_a_warning_and_the_scan_continues() {
    let dir = fixture();
    fs::write(dir.path().join("docs/bad.md"), "POISON should abort only this file").unwrap();

    let out = run_on_file(&dir, "SELECT title FROM './**/*.md'", "./parse.sh {path}");

    // Per-file isolation: the good files still return.
    assert_eq!(titles(&out), vec!["alpha title", "bravo title"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("bad.md"),
        "the skipped file must be named on stderr, got: {stderr}"
    );
}

#[test]
fn a_repeated_on_file_flag_is_an_error_pointing_at_config_files() {
    let dir = fixture();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg("SELECT title FROM './**/*.md'")
        .arg("--on-file")
        .arg("./parse.sh {path}")
        .arg("--on-file")
        .arg("cat {path}")
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed");

    assert!(!out.status.success(), "a repeated --on-file must be rejected");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("config"),
        "the error must point at config files for multi-table setups, got: {stderr}"
    );
}

#[test]
fn a_parsed_scan_honors_the_default_ignore_rules() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
    fs::write(
        dir.path().join("node_modules/pkg/dep.md"),
        "dependency title\nnoise",
    )
    .unwrap();

    let out = run_on_file(&dir, "SELECT title FROM './**/*.md'", "./parse.sh {path}");

    assert!(
        !titles(&out).contains(&"dependency title".to_string()),
        "node_modules must be skipped by a parsed scan too: {:?}",
        titles(&out)
    );
    assert_eq!(titles(&out), vec!["alpha title", "bravo title"]);
}
