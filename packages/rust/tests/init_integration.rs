//! Integration tests for `dirsql init`.
//!
//! `init` writes the exact same starter config every time -- the same
//! single `files` table zero-config mode would serve -- so a user has
//! something loadable to hand-edit. It does not inspect the target
//! directory's contents at all: no LLM, no network, no filesystem walk.
//! CI-runnable.
//!
//! Gated behind `--features cli` (the binary is feature-gated).

#![cfg(feature = "cli")]

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

/// The exact, fixed starter config `init` writes -- byte-for-byte the same
/// `[[table]]` block the zero-config server/CLI default (`default_files_table`
/// in `src/bin/dirsql.rs`) uses.
const EXPECTED_TOML: &str = "[[table]]\nddl  = \"CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER, _ctime INTEGER)\"\nglob = \"**/*\"\n";

fn run_init(cwd: &Path, extra_args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("init")
        .args(extra_args)
        .current_dir(cwd)
        .output()
        .expect("spawning dirsql failed")
}

#[test]
fn writes_the_fixed_default_files_table_config() {
    let cwd = TempDir::new().unwrap();

    let output = run_init(cwd.path(), &[]);
    assert!(
        output.status.success(),
        "init failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let toml = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_eq!(toml, EXPECTED_TOML);

    dirsql::DirSQL::from_config_path(cwd.path().join(".dirsql.toml"))
        .expect("config produced by `dirsql init` must load via from_config_path");
}

#[test]
fn output_is_identical_regardless_of_directory_contents() {
    // `init` never inspects the target directory -- an empty directory and a
    // directory full of mixed files (including a genuinely binary one) must
    // produce byte-identical output.
    let empty = TempDir::new().unwrap();
    let mixed = TempDir::new().unwrap();
    fs::write(mixed.path().join("notes.txt"), "hello world\n").unwrap();
    fs::write(mixed.path().join("data.json"), r#"{"a": 1}"#).unwrap();
    fs::write(
        mixed.path().join("photo.jpg"),
        [0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10],
    )
    .unwrap();
    fs::create_dir(mixed.path().join("nested")).unwrap();
    fs::write(mixed.path().join("nested").join("a.md"), "hi").unwrap();

    let empty_out = run_init(empty.path(), &[]);
    let mixed_out = run_init(mixed.path(), &[]);
    assert!(empty_out.status.success());
    assert!(mixed_out.status.success());

    let empty_toml = fs::read_to_string(empty.path().join(".dirsql.toml")).unwrap();
    let mixed_toml = fs::read_to_string(mixed.path().join(".dirsql.toml")).unwrap();
    assert_eq!(empty_toml, mixed_toml);
    assert_eq!(empty_toml, EXPECTED_TOML);
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
    assert_eq!(written, EXPECTED_TOML);
}

#[test]
fn root_flag_targets_a_different_directory() {
    let cwd = TempDir::new().unwrap();
    let scan_root = TempDir::new().unwrap();

    let output = run_init(
        cwd.path(),
        &["--root", scan_root.path().to_str().unwrap()],
    );
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
