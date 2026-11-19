//! A config glob passed with `-c` anchors at the config's directory, `../`
//! included.
//!
//! Gated behind `--features cli` like the sibling CLI e2e tests.
#![cfg(feature = "cli")]

use std::fs;

use assert_cmd::prelude::*;
use tempfile::TempDir;

const HOOK: &str = r#"on-file = '''sh -c 'for p; do p=$(printf %s "$p" | tr "\\\\" /); printf "{\"basename\":\"%s\"}\n" "${p##*/}"; done' sh'''"#;

#[test]
fn a_parent_relative_config_glob_matches_files_above_the_config() {
    let dir = TempDir::new().unwrap();
    let cfg_dir = dir.path().join("cfg");
    fs::create_dir(&cfg_dir).unwrap();
    fs::create_dir(dir.path().join("data")).unwrap();
    fs::write(dir.path().join("data/a.json"), "{}").unwrap();
    let cfg = cfg_dir.join(".dirsql.toml");
    fs::write(
        &cfg,
        format!(
            "[[table]]\nname = \"docs\"\nddl = \"CREATE TABLE docs (basename TEXT)\"\nglob = '../data/*.json'\n{HOOK}\n"
        ),
    )
    .unwrap();

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli")
        .arg("query")
        .arg("SELECT basename FROM docs")
        .arg("-c")
        .arg(&cfg)
        .current_dir(&cfg_dir)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr: {stderr}");
    assert!(stdout.contains("a.json"), "stdout: {stdout}\nstderr: {stderr}");
}
