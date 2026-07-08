//! Integration tests for `dirsql init`.
//!
//! `init` is deterministic: it writes the fixed
//! [`dirsql::cli::DEFAULT_CONFIG_TOML`] asset verbatim and never inspects the
//! target directory. These tests spawn the compiled `dirsql` binary as a
//! subprocess and exercise flag parsing, file writing, and --force —
//! no stubs, no live LLM, CI-runnable.
//!
//! Gated behind `--features cli` (the binary is feature-gated).

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;

use assert_cmd::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

fn run_init(cwd: &std::path::Path, extra_args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("init")
        .args(extra_args)
        .current_dir(cwd)
        .output()
        .expect("spawning dirsql failed")
}

#[test]
fn writes_the_default_config_verbatim() {
    let cwd = TempDir::new().unwrap();
    let output = run_init(cwd.path(), &[]);
    assert!(
        output.status.success(),
        "init failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let config_path = cwd.path().join(".dirsql.toml");
    let written = fs::read_to_string(&config_path).unwrap();
    assert_eq!(
        written,
        dirsql::cli::DEFAULT_CONFIG_TOML,
        "init must write DEFAULT_CONFIG_TOML verbatim",
    );

    dirsql::DirSQL::from_config_path(&config_path)
        .expect("config produced by `dirsql init` must load via from_config_path");
}

#[test]
fn ignores_target_directory_contents() {
    // A directory full of files must not change what `init` writes — it does
    // not inspect the target at all.
    let cwd = TempDir::new().unwrap();
    fs::write(cwd.path().join("a.txt"), "hello").unwrap();
    fs::write(cwd.path().join("b.json"), "{}").unwrap();
    fs::create_dir(cwd.path().join("sub")).unwrap();
    fs::write(cwd.path().join("sub/c.rs"), "fn main() {}").unwrap();

    let output = run_init(cwd.path(), &[]);
    assert!(output.status.success(), "init failed: {output:?}");

    let written = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_eq!(written, dirsql::cli::DEFAULT_CONFIG_TOML);
}

#[test]
fn refuses_to_overwrite_existing_config() {
    let cwd = TempDir::new().unwrap();
    let first = run_init(cwd.path(), &[]);
    assert!(
        first.status.success(),
        "baseline init must succeed: stderr={}",
        String::from_utf8_lossy(&first.stderr),
    );
    let written = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();

    let second = run_init(cwd.path(), &[]);
    assert!(
        !second.status.success(),
        "second init must fail when .dirsql.toml already exists",
    );
    let preserved = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_eq!(
        preserved, written,
        "existing config must not be modified by the failed run",
    );
}

#[test]
fn force_flag_overwrites_existing_config() {
    let cwd = TempDir::new().unwrap();
    fs::write(cwd.path().join(".dirsql.toml"), "# old\n").unwrap();

    let output = run_init(cwd.path(), &["--force"]);
    assert!(
        output.status.success(),
        "init --force failed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let written = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_eq!(written, dirsql::cli::DEFAULT_CONFIG_TOML);
    dirsql::DirSQL::from_config_path(cwd.path().join(".dirsql.toml"))
        .expect("forced-overwrite config must load");
}

#[test]
fn root_flag_targets_a_different_directory() {
    let cwd = TempDir::new().unwrap();
    let scan_root = TempDir::new().unwrap();

    let output = run_init(cwd.path(), &["--root", scan_root.path().to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {output:?}");

    // Per docs: default --output is `<root>/.dirsql.toml`.
    assert!(
        scan_root.path().join(".dirsql.toml").exists(),
        ".dirsql.toml should land in --root, not cwd",
    );
    assert!(
        !cwd.path().join(".dirsql.toml").exists(),
        "cwd should not be written when --root is set",
    );
}

#[test]
fn output_flag_redirects_destination() {
    let cwd = TempDir::new().unwrap();
    let custom_out = cwd.path().join("custom.toml");

    let output = run_init(cwd.path(), &["--output", custom_out.to_str().unwrap()]);
    assert!(output.status.success(), "init failed: {output:?}");

    assert!(custom_out.exists(), "config should land at --output path");
    assert!(
        !cwd.path().join(".dirsql.toml").exists(),
        "default path must not be written when --output is set",
    );
}

#[test]
fn init_help_lists_documented_flags() {
    std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .arg("init")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("--root"))
        .stdout(contains("--output"))
        .stdout(contains("--force"));
}
