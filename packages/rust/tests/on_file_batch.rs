//! Integration tests for the one-process-per-table `on-file` contract: the
//! command runs once per table with every matched absolute path appended as
//! trailing arguments, and prints one JSON array of row objects.
//!
//! Unix-only: the fixtures shell out to `sh`. The Rust CI test job runs on Linux.
#![cfg(unix)]

use std::fs;

use dirsql::DirSQL;
use tempfile::TempDir;

/// The script appends one line per invocation to `argv.log`, each line being
/// its arguments joined by tabs, so the log is a transcript of every spawn.
#[test]
fn on_file_runs_once_per_table_with_every_matched_path_as_trailing_args() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("record.sh"),
        "#!/bin/sh\nline=\"\"\nfor arg; do line=\"$line$arg\t\"; done\nprintf '%s\\n' \"$line\" >> argv.log\nprintf '[]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh record.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "a\n").unwrap();
    fs::write(root.path().join("b.txt"), "b\n").unwrap();
    fs::write(root.path().join("c.txt"), "c\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    assert!(db.scan_failures().is_empty(), "{:?}", db.scan_failures());

    let log = fs::read_to_string(root.path().join("argv.log")).unwrap_or_default();
    let invocations: Vec<&str> = log.lines().collect();
    assert_eq!(
        invocations.len(),
        1,
        "the command runs exactly once per table, got: {log:?}"
    );

    let args: Vec<&str> = invocations[0]
        .split('\t')
        .filter(|arg| !arg.is_empty())
        .collect();
    let canonical = root.path().canonicalize().unwrap();
    let expected: Vec<String> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|name| canonical.join(name).to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args, expected,
        "every matched absolute path is a trailing argument"
    );
}
