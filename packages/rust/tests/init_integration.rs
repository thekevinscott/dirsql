//! Integration tests for `dirsql init` (issue #96).
//!
//! These tests spawn the compiled `dirsql` binary as a subprocess but
//! replace `claude` with a stub shell script that prints a canned
//! `.dirsql.toml` to stdout. That keeps the CLI behavior under test —
//! flag parsing, file writing, --force, missing-claude error path —
//! while skipping the live LLM call. CI-runnable.
//!
//! See `init_e2e.rs` for the real-`claude` variant, which is local-only.
//!
//! Gated behind `--features cli` (the binary is feature-gated) and
//! `unix` (the stub is a shell script).

#![cfg(all(feature = "cli", unix))]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use predicates::str::contains;
use tempfile::TempDir;

/// A minimal filesystem-fact config the stub returns. Loadable by
/// `DirSQL::from_config_path`.
const CANNED_TOML: &str = r#"[[table]]
ddl  = "CREATE TABLE files (_path TEXT)"
glob = "*"
"#;

/// Write a `claude` stub into `dir` that prints `response` on stdout and
/// exits zero. Touches `sentinel` so tests can assert the stub was
/// invoked. The stub ignores its arguments.
fn write_stub_claude(dir: &Path, response: &str, sentinel: &Path) {
    let stub = dir.join("claude");
    let script = format!(
        "#!/bin/sh\ntouch '{}'\ncat <<'__DIRSQL_TOML__'\n{response}__DIRSQL_TOML__\n",
        sentinel.display(),
    );
    fs::write(&stub, script).unwrap();
    let mut perms = fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub, perms).unwrap();
}

/// Write a `claude` stub that exits non-zero with `stderr_msg` on stderr.
/// Touches `sentinel` so tests can assert the stub was reached before
/// the failure.
fn write_failing_stub_claude(dir: &Path, exit_code: i32, stderr_msg: &str, sentinel: &Path) {
    let stub = dir.join("claude");
    let escaped = stderr_msg.replace('\'', "'\\''");
    let script = format!(
        "#!/bin/sh\ntouch '{}'\nprintf '%s\\n' '{escaped}' >&2\nexit {exit_code}\n",
        sentinel.display(),
    );
    fs::write(&stub, script).unwrap();
    let mut perms = fs::metadata(&stub).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&stub, perms).unwrap();
}

/// Run `dirsql init` with `stub_dir` prepended to `PATH` so the stub
/// `claude` wins over any system `claude`.
fn run_init(cwd: &Path, stub_dir: &Path, extra_args: &[&str]) -> Output {
    let original_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", stub_dir.display(), original_path);
    std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli")
        .arg("init")
        .args(extra_args)
        .current_dir(cwd)
        .env("PATH", new_path)
        .output()
        .expect("spawning dirsql failed")
}

// ---------------------------------------------------------------------------
// Happy path
// ---------------------------------------------------------------------------

#[test]
fn writes_a_loadable_dirsql_config() {
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_stub_claude(stub_dir.path(), CANNED_TOML, &sentinel);

    let cwd = TempDir::new().unwrap();
    let output = run_init(cwd.path(), stub_dir.path(), &[]);
    assert!(
        output.status.success(),
        "init failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );

    let config_path = cwd.path().join(".dirsql.toml");
    assert!(config_path.exists(), "expected .dirsql.toml to be written");

    dirsql::DirSQL::from_config_path(&config_path)
        .expect("config produced by `dirsql init` must load via from_config_path");
}

// ---------------------------------------------------------------------------
// --force semantics
// ---------------------------------------------------------------------------

#[test]
fn refuses_to_overwrite_existing_config() {
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_stub_claude(stub_dir.path(), CANNED_TOML, &sentinel);

    // First run: empty cwd, the happy-path baseline. Establishes that
    // init can in fact write a config, so the second-run failure below
    // can't be confused with "init is broken".
    let cwd = TempDir::new().unwrap();
    let first = run_init(cwd.path(), stub_dir.path(), &[]);
    assert!(
        first.status.success(),
        "baseline init must succeed: stderr={}",
        String::from_utf8_lossy(&first.stderr),
    );
    let written = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert!(
        sentinel.exists(),
        "claude must have been invoked on first run"
    );

    // Reset sentinel; second run must fail without touching the file.
    fs::remove_file(&sentinel).unwrap();

    let second = run_init(cwd.path(), stub_dir.path(), &[]);
    assert!(
        !second.status.success(),
        "second init must fail when .dirsql.toml already exists",
    );
    let preserved = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_eq!(
        preserved, written,
        "existing config must not be modified by the failed run",
    );
    assert!(
        !sentinel.exists(),
        "claude must not be invoked when output already exists and --force is absent",
    );
}

#[test]
fn force_flag_overwrites_existing_config() {
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_stub_claude(stub_dir.path(), CANNED_TOML, &sentinel);

    let cwd = TempDir::new().unwrap();
    fs::write(cwd.path().join(".dirsql.toml"), "# old\n").unwrap();

    let output = run_init(cwd.path(), stub_dir.path(), &["--force"]);
    assert!(
        output.status.success(),
        "init --force failed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );

    let written = fs::read_to_string(cwd.path().join(".dirsql.toml")).unwrap();
    assert_ne!(written, "# old\n", "config must be replaced");
    dirsql::DirSQL::from_config_path(cwd.path().join(".dirsql.toml"))
        .expect("forced-overwrite config must load");
}

// ---------------------------------------------------------------------------
// --root and --output flags
// ---------------------------------------------------------------------------

#[test]
fn root_flag_targets_a_different_directory() {
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_stub_claude(stub_dir.path(), CANNED_TOML, &sentinel);

    let cwd = TempDir::new().unwrap();
    let scan_root = TempDir::new().unwrap();

    let output = run_init(
        cwd.path(),
        stub_dir.path(),
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
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_stub_claude(stub_dir.path(), CANNED_TOML, &sentinel);

    let cwd = TempDir::new().unwrap();
    let custom_out = cwd.path().join("custom.toml");

    let output = run_init(
        cwd.path(),
        stub_dir.path(),
        &["--output", custom_out.to_str().unwrap()],
    );
    assert!(output.status.success(), "init failed: {output:?}");

    assert!(custom_out.exists(), "config should land at --output path");
    assert!(
        !cwd.path().join(".dirsql.toml").exists(),
        "default path must not be written when --output is set",
    );
}

// ---------------------------------------------------------------------------
// Failure paths
// ---------------------------------------------------------------------------

#[test]
fn raises_when_claude_is_missing() {
    // Empty stub_dir + restricted PATH so `claude` cannot be resolved at all.
    let stub_dir = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();

    let output = std::process::Command::cargo_bin("dirsql")
        .unwrap()
        .arg("init")
        .current_dir(cwd.path())
        .env("PATH", stub_dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "init should fail when `claude` is not on PATH",
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("claude"),
        "error must mention `claude`: stderr={stderr}",
    );

    assert!(
        !cwd.path().join(".dirsql.toml").exists(),
        "no config must be written when `claude` is unavailable",
    );
}

#[test]
fn does_not_write_partial_config_when_claude_fails() {
    let stub_dir = TempDir::new().unwrap();
    let sentinel = stub_dir.path().join("claude.called");
    write_failing_stub_claude(stub_dir.path(), 1, "boom", &sentinel);

    let cwd = TempDir::new().unwrap();
    let output = run_init(cwd.path(), stub_dir.path(), &[]);
    assert!(
        !output.status.success(),
        "init should fail when `claude` exits non-zero",
    );

    // Sentinel proves init actually invoked claude (rather than failing
    // before reaching the subprocess); only then is the
    // "no partial file" guarantee meaningful.
    assert!(
        sentinel.exists(),
        "init must have invoked claude before failing",
    );
    assert!(
        !cwd.path().join(".dirsql.toml").exists(),
        "no config must be written when `claude` fails",
    );
}

// ---------------------------------------------------------------------------
// --help surface (every documented flag must appear)
// ---------------------------------------------------------------------------

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
