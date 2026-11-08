//! End-to-end: the real `dirsql` binary lists a file whose absolute path is
//! longer than the platform's path limit, with its stat columns and content.
//! Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

use std::fs;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const LEVELS: usize = 30;

/// `LEVELS` nested directories with 200-character names, one file in each,
/// so the deepest files sit far past a 4096-byte absolute path. Built from
/// the bottom up, renaming the chain so far into each new parent, because no
/// call may name a path past the limit.
fn deep_tree() -> TempDir {
    let staging = TempDir::new().unwrap();
    let name = "D".repeat(200);
    let chain = staging.path().join("chain");
    let parent = staging.path().join("parent");
    for level in (1..=LEVELS).rev() {
        fs::create_dir(&parent).unwrap();
        fs::write(parent.join(format!("leaf{level:02}.md")), "x").unwrap();
        if level < LEVELS {
            fs::rename(&chain, parent.join(&name)).unwrap();
        }
        fs::rename(&parent, &chain).unwrap();
    }
    let root = TempDir::new().unwrap();
    fs::rename(&chain, root.path().join(&name)).unwrap();
    root
}

#[test]
fn a_double_star_keeps_files_past_the_path_limit() {
    let root = deep_tree();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("SELECT COUNT(*) n, COUNT(size) sized, COUNT(content) read FROM './**'")
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
        "[{\"n\":30,\"sized\":30,\"read\":30}]\n"
    );
}
