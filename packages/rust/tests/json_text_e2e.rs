//! End-to-end tests for text cells in the CLI's JSON output: the real
//! `dirsql` binary must print each text cell as the JSON string
//! `serde_json` writes for it, reading text SQLite holds as invalid UTF-8
//! with replacement characters.
//!
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn json(sql: &str) -> String {
    let root = TempDir::new().unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg(sql)
        .args(["--format", "json"])
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("stdout must be UTF-8")
}

#[test]
fn text_that_is_not_utf8_prints_with_replacement_characters() {
    assert_eq!(
        json("SELECT CAST(x'ff41' AS TEXT) AS t"),
        "[{\"t\":\"\u{FFFD}A\"}]\n"
    );
}

#[test]
fn text_needing_escapes_prints_with_json_escapes() {
    assert_eq!(
        json("SELECT char(0, 9, 10, 31, 34, 92, 127, 233, 8232) AS c, 'docs/a b.md' AS p"),
        "[{\"c\":\"\\u0000\\t\\n\\u001f\\\"\\\\\u{7f}\u{e9}\u{2028}\",\"p\":\"docs/a b.md\"}]\n"
    );
}
