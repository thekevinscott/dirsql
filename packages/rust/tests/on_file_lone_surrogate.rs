//! A lone surrogate escape in an `on-file` hook's output fails the table with
//! an error naming the line and the escape.

use std::fs;

use dirsql::{DirSQL, DirSqlError};
use tempfile::TempDir;

fn build_error(line: &str) -> String {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (a TEXT)\"\nglob = \"*.ndjson\"\non-file = \"cat\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join("s.ndjson"),
        format!("{{\"a\":\"ok\"}}\n{line}\n"),
    )
    .unwrap();
    let err = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .err()
        .expect("a lone surrogate must fail the build");
    match err {
        DirSqlError::TableCommand { message, .. } => message,
        other => panic!("expected a table command failure, got: {other}"),
    }
}

#[test]
fn a_lone_leading_surrogate_names_the_line_and_the_escape() {
    let message = build_error(r#"{"a":"\ud800"}"#);
    assert!(
        message.contains(r"line 2: lone surrogate \ud800 is not valid Unicode"),
        "got: {message}"
    );
}

#[test]
fn a_lone_trailing_surrogate_names_the_line_and_the_escape() {
    let message = build_error(r#"{"a":"x\uDC00y"}"#);
    assert!(
        message.contains(r"line 2: lone surrogate \uDC00 is not valid Unicode"),
        "got: {message}"
    );
}

#[test]
fn a_lone_surrogate_in_a_key_names_the_escape() {
    let message = build_error(r#"{"\udbff":1}"#);
    assert!(
        message.contains(r"line 2: lone surrogate \udbff is not valid Unicode"),
        "got: {message}"
    );
}

#[test]
fn an_escaped_backslash_before_u_is_not_a_surrogate() {
    let message = build_error(r#"{"a":"\\ud800","b":"\ud83d\ude00","c":"\udfff"}"#);
    assert!(
        message.contains(r"line 2: lone surrogate \udfff is not valid Unicode"),
        "got: {message}"
    );
}
