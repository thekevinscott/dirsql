//! End-to-end: the real `dirsql` binary reports a pre-1970 `mtime` as
//! negative Unix seconds, not NULL.
//!
//! Nothing is mocked (real process, real filesystem, real SQLite).
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use assert_cmd::prelude::*;
use tempfile::TempDir;

#[test]
fn a_pre_epoch_mtime_prints_as_negative_seconds() {
    let root = TempDir::new().unwrap();
    let file = root.path().join("neg.md");
    fs::write(&file, "x").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(UNIX_EPOCH - Duration::from_secs(86_400))
        .unwrap();

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("SELECT path, mtime FROM './neg.md'")
        .args(["--format", "json"])
        .env("DIRSQL_PROGRESS", "never")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql` failed");

    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "[{\"path\":\"neg.md\",\"mtime\":-86400}]\n"
    );
}
