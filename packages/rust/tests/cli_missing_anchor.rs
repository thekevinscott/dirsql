//! A config table whose own directory does not exist fails the query, naming
//! the table and the directory.
//!
//! Gated behind `--features cli` like the sibling CLI e2e tests.
#![cfg(feature = "cli")]

use std::fs;

use assert_cmd::prelude::*;
use tempfile::TempDir;

#[test]
fn a_config_table_whose_directory_is_missing_fails_the_query() {
    let dir = TempDir::new().unwrap();
    let cfg_dir = dir.path().join("cfg");
    fs::create_dir(&cfg_dir).unwrap();
    let cfg = cfg_dir.join(".dirsql.toml");
    fs::write(
        &cfg,
        "[[table]]\nname = \"later\"\nddl = \"CREATE TABLE later (x TEXT)\"\nglob = \"../absent/*.json\"\non-file = \"true\"\n",
    )
    .unwrap();

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli")
        .arg("query")
        .arg("SELECT COUNT(*) AS n FROM later")
        .arg("-c")
        .arg(&cfg)
        .current_dir(&cfg_dir)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "exited zero; stderr: {stderr}");
    assert!(
        stderr.contains("table 'later': its directory ")
            && stderr.contains("absent does not exist"),
        "{stderr}"
    );
}
