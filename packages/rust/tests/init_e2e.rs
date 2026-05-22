//! End-to-end tests for `dirsql init`.
//!
//! These tests invoke the **real** `claude` binary against a real fixture
//! directory and assert that the produced `.dirsql.toml` is a loadable
//! config. Because they incur live LLM calls and require a signed-in
//! `claude`, they are NOT run in CI: each test skips with an
//! eprintln-warning if `claude` is not on `PATH`.
//!
//! Run locally with:
//!   cargo test -p dirsql --features cli --test init_e2e
//!
//! Gated behind `--features cli` (the binary is feature-gated).

#![cfg(feature = "cli")]

use std::fs;
use std::process::{Command, Stdio};

use assert_cmd::prelude::*;
use tempfile::TempDir;

/// Returns true when a working `claude` binary is on `PATH`. Tests use this
/// to skip rather than fail when the e2e prerequisite is unavailable
/// (CI / hosted sandboxes / fresh dev machines).
fn claude_available() -> bool {
    Command::new("claude")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn skip_if_no_claude(test_name: &str) -> bool {
    if !claude_available() {
        eprintln!(
            "[skip] {test_name}: `claude` not on PATH (e2e tests require a signed-in claude CLI)"
        );
        return true;
    }
    false
}

#[test]
fn produces_a_loadable_config_for_a_mixed_directory() {
    if skip_if_no_claude("produces_a_loadable_config_for_a_mixed_directory") {
        return;
    }

    // Mixed-content fixture: plain-text and JSON files for `claude` to model.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("notes.txt"), "hello world\n").unwrap();
    fs::write(root.path().join("more.txt"), "another note\n").unwrap();
    fs::write(
        root.path().join("data.json"),
        r#"{"vendor": "Acme", "amount": 42}"#,
    )
    .unwrap();

    // Live agent calls can take tens of seconds; the harness's overall
    // test timeout governs the upper bound here.
    let output = Command::cargo_bin("dirsql")
        .unwrap()
        .arg("init")
        .current_dir(root.path())
        .output()
        .expect("spawning dirsql failed");

    assert!(
        output.status.success(),
        "init failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let config = root.path().join(".dirsql.toml");
    assert!(config.exists(), "init must write .dirsql.toml");

    dirsql::DirSQL::from_config_path(&config)
        .expect("`dirsql init` output must round-trip through DirSQL::from_config_path");
}
