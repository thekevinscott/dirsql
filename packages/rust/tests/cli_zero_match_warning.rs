//! A config `[[table]]` whose glob matches no files warns on stderr, naming the
//! table, the glob and the anchor. The exit code is unchanged.
//!
//! Gated behind `--features cli` like the sibling CLI e2e tests.
#![cfg(feature = "cli")]

use std::fs;
use std::path::Path;

use assert_cmd::prelude::*;
use tempfile::TempDir;

fn write_config(dir: &Path, glob: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(
        &path,
        format!(
            "[[table]]\nname = \"sessions\"\nddl = \"CREATE TABLE sessions (n TEXT)\"\nglob = \"{glob}\"\non-file = \"printf []\"\n"
        ),
    )
    .unwrap();
    path
}

fn query(cwd: &Path, config: &Path) -> std::process::Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli")
        .arg("query")
        .arg("SELECT COUNT(*) AS n FROM sessions")
        .arg("-c")
        .arg(config)
        .current_dir(cwd)
        .output()
        .unwrap()
}

#[test]
fn an_empty_config_table_warns_on_stderr_and_still_exits_zero() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_dir_path = fs::canonicalize(cfg_dir.path()).unwrap();
    let config = write_config(&cfg_dir_path, "projects/*/*.jsonl");
    let elsewhere = TempDir::new().unwrap();

    let out = query(elsewhere.path(), &config);

    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let expected = format!(
        "dirsql: table 'sessions': glob 'projects/*/*.jsonl' matched no files under {}",
        cfg_dir_path.display()
    );
    assert!(
        stderr.lines().any(|line| line == expected),
        "expected {expected:?} in stderr, got {stderr:?}"
    );
}

#[test]
fn a_config_table_with_matches_does_not_warn() {
    let cfg_dir = TempDir::new().unwrap();
    fs::create_dir_all(cfg_dir.path().join("projects/p")).unwrap();
    fs::write(cfg_dir.path().join("projects/p/a.jsonl"), "{}").unwrap();
    let config = write_config(cfg_dir.path(), "projects/*/*.jsonl");
    let elsewhere = TempDir::new().unwrap();

    let out = query(elsewhere.path(), &config);

    assert!(out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("matched no files"), "got {stderr:?}");
}
