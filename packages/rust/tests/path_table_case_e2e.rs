//! CLI e2e for path-tables whose spellings differ only in letter case: the
//! real `dirsql` binary, a real temp directory, nothing mocked.

#![cfg(feature = "cli")]

use std::fs;
use std::io::Write;
use std::process::{Output, Stdio};

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use tempfile::TempDir;

/// `a.md` and `b.MD`: names that coexist on a case-insensitive filesystem too,
/// told apart only by globs that differ in case.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "lower").unwrap();
    fs::write(root.path().join("b.MD"), "upper").unwrap();
    root
}

fn dirsql(dir: &TempDir, args: &[&str], stdin: &str) -> Output {
    let mut child = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawning `dirsql` failed");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes())
        .expect("writing to stdin failed");
    let out = child
        .wait_with_output()
        .expect("waiting on `dirsql` failed");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn json_lines(out: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line must be JSON"))
        .collect()
}

#[test]
fn a_query_reads_both_case_variants_as_distinct_tables() {
    let root = fixture();

    let out = dirsql(
        &root,
        &[
            "query",
            "SELECT path FROM './*.md' UNION ALL SELECT path FROM './*.MD'",
        ],
        "",
    );

    assert_eq!(
        json_lines(&out),
        vec![json!([{"path": "a.md"}, {"path": "b.MD"}])]
    );
}

#[test]
fn a_join_of_case_variants_pairs_their_own_rows() {
    let root = fixture();

    let out = dirsql(
        &root,
        &[
            "query",
            "SELECT l.path AS L, u.path AS U FROM './*.md' l, './*.MD' u",
        ],
        "",
    );

    assert_eq!(json_lines(&out), vec![json!([{"L": "a.md", "U": "b.MD"}])]);
}

#[test]
fn a_repl_session_reads_each_case_variant_afresh() {
    let root = fixture();

    let out = dirsql(
        &root,
        &[],
        "SELECT path FROM './*.md'\nSELECT path FROM './*.MD'\nSELECT path FROM './*.md'\n",
    );

    assert_eq!(
        json_lines(&out),
        vec![
            json!([{"path": "a.md"}]),
            json!([{"path": "b.MD"}]),
            json!([{"path": "a.md"}]),
        ]
    );
}
