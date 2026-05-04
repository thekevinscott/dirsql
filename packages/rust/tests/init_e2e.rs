//! End-to-end tests for the `dirsql init` subcommand (issue #96).
//!
//! These tests spawn the actual compiled `dirsql` binary, invoke the
//! `init` subcommand against a real fixture directory, and assert on
//! the contents of the resulting `.dirsql.toml` and the binary's
//! exit code / stdout. Nothing is mocked — the LLM call itself is
//! tested via the offline `--apply` mode that consumes a JSON
//! response from a file, which is the same code path the in-process
//! HTTP client would feed.
//!
//! Gated behind `--features cli` (the `dirsql` bin target requires it).

#![cfg(feature = "cli")]

use std::fs;
use std::process::{Command as StdCommand, Stdio};

use assert_cmd::prelude::*;
use predicates::prelude::*;
use tempfile::TempDir;

/// Write a small mixed-format fixture: posts/*.json, data/*.csv, notes/*.md
/// plus some noise that should be ignored by default.
fn mixed_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::write(
        root.path().join("posts/hello.json"),
        r#"{"title":"Hello","author":"alice"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("posts/second.json"),
        r#"{"title":"Two","author":"bob"}"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data/users.csv"), "name,age\nalice,30\n").unwrap();
    fs::create_dir_all(root.path().join("notes")).unwrap();
    fs::write(
        root.path().join("notes/intro.md"),
        "---\ntitle: Intro\n---\nbody",
    )
    .unwrap();
    // Noise that must not appear in the generated config.
    fs::create_dir_all(root.path().join("node_modules/x")).unwrap();
    fs::write(root.path().join("node_modules/x/pkg.json"), "{}").unwrap();
    fs::create_dir_all(root.path().join(".git")).unwrap();
    fs::write(root.path().join(".git/HEAD"), "ref: refs/heads/main").unwrap();
    root
}

fn cargo_bin_dirsql() -> StdCommand {
    StdCommand::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test --features cli`")
}

// ---------------------------------------------------------------------------
// `dirsql init` (template mode)
// ---------------------------------------------------------------------------

#[test]
fn init_template_writes_dirsql_toml_with_observed_extensions() {
    let root = mixed_fixture();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .current_dir(root.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    cmd.assert().success();

    let toml = fs::read_to_string(root.path().join(".dirsql.toml"))
        .expect(".dirsql.toml must be created at the project root");

    // Tables for json/csv/md must be present; noise dirs must NOT be
    // surfaced as table globs (they may legitimately appear in the
    // [dirsql].ignore array).
    assert!(toml.contains("posts/*.json"), "{toml}");
    assert!(toml.contains("data/*.csv"), "{toml}");
    assert!(toml.contains("notes/*.md"), "{toml}");

    // The output must round-trip through the actual config loader.
    let cfg = dirsql::config::load_config_str(&toml).expect("rendered toml must parse");
    assert!(cfg.tables.iter().any(|t| t.glob == "posts/*.json"));
    assert!(cfg.tables.iter().any(|t| t.glob == "data/*.csv"));
    assert!(cfg.tables.iter().any(|t| t.glob == "notes/*.md"));
    // Noise dirs must never become a table glob.
    for t in &cfg.tables {
        assert!(
            !t.glob.contains("node_modules") && !t.glob.contains(".git"),
            "table glob {:?} should not match noise dirs",
            t.glob
        );
    }
}

#[test]
fn init_refuses_to_overwrite_existing_config() {
    let root = mixed_fixture();
    fs::write(root.path().join(".dirsql.toml"), "# user-authored").unwrap();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init").current_dir(root.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("--force"));

    assert_eq!(
        fs::read_to_string(root.path().join(".dirsql.toml")).unwrap(),
        "# user-authored",
        "init must not overwrite without --force"
    );
}

#[test]
fn init_force_overwrites_existing_config() {
    let root = mixed_fixture();
    fs::write(root.path().join(".dirsql.toml"), "# user-authored").unwrap();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init").arg("--force").current_dir(root.path());
    cmd.assert().success();

    let toml = fs::read_to_string(root.path().join(".dirsql.toml")).unwrap();
    assert!(toml.contains("posts/*.json"), "{toml}");
}

#[test]
fn init_output_flag_redirects_destination() {
    let root = mixed_fixture();
    let dest = root.path().join("custom.toml");

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .arg("--output")
        .arg(&dest)
        .current_dir(root.path());
    cmd.assert().success();

    assert!(dest.exists());
    assert!(!root.path().join(".dirsql.toml").exists());
    let toml = fs::read_to_string(&dest).unwrap();
    assert!(toml.contains("posts/*.json"), "{toml}");
}

#[test]
fn init_root_flag_scans_other_directory() {
    // Run from `elsewhere`, point --root at the fixture; expect the file
    // to appear inside the fixture (so that `dirsql` later reads it from
    // the same root).
    let fixture = mixed_fixture();
    let elsewhere = TempDir::new().unwrap();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .arg("--root")
        .arg(fixture.path())
        .current_dir(elsewhere.path());
    cmd.assert().success();

    assert!(fixture.path().join(".dirsql.toml").exists());
    assert!(!elsewhere.path().join(".dirsql.toml").exists());
}

// ---------------------------------------------------------------------------
// `dirsql init --infer --print-prompt`
// ---------------------------------------------------------------------------

#[test]
fn init_infer_print_prompt_emits_directory_summary_to_stdout() {
    let root = mixed_fixture();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .arg("--infer")
        .arg("--print-prompt")
        .current_dir(root.path());
    cmd.assert()
        .success()
        // The prompt must mention the schema contract (so the LLM
        // produces JSON we can parse) and at least one observed glob.
        .stdout(predicate::str::contains("\"tables\""))
        .stdout(predicate::str::contains("posts/*.json"));

    // --print-prompt must NOT write `.dirsql.toml`.
    assert!(!root.path().join(".dirsql.toml").exists());
}

// ---------------------------------------------------------------------------
// `dirsql init --infer --apply <file>`
// ---------------------------------------------------------------------------

#[test]
fn init_infer_apply_writes_config_from_llm_response_file() {
    let root = mixed_fixture();
    let response_path = root.path().join("llm_response.json");
    fs::write(
        &response_path,
        r#"{
          "ignore": ["node_modules/**"],
          "tables": [
            {"ddl": "CREATE TABLE posts (title TEXT, author TEXT)", "glob": "posts/*.json"}
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .arg("--infer")
        .arg("--apply")
        .arg(&response_path)
        .current_dir(root.path());
    cmd.assert().success();

    let toml = fs::read_to_string(root.path().join(".dirsql.toml")).unwrap();
    let cfg = dirsql::config::load_config_str(&toml).unwrap();
    assert_eq!(cfg.tables.len(), 1);
    assert_eq!(cfg.tables[0].glob, "posts/*.json");
    assert_eq!(cfg.ignore, vec!["node_modules/**".to_string()]);
}

#[test]
fn init_infer_apply_rejects_response_without_tables() {
    let root = mixed_fixture();
    let response_path = root.path().join("llm_response.json");
    fs::write(&response_path, r#"{"ignore": []}"#).unwrap();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init")
        .arg("--infer")
        .arg("--apply")
        .arg(&response_path)
        .current_dir(root.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("tables"));

    // Failure must NOT leave a partial file behind.
    assert!(!root.path().join(".dirsql.toml").exists());
}

#[test]
fn init_infer_without_subflag_errors_with_helpful_message() {
    // Bare `init --infer` (no --print-prompt, no --apply) should not
    // silently succeed — there's no built-in HTTP client yet, so we
    // tell the user how to proceed.
    let root = mixed_fixture();

    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init").arg("--infer").current_dir(root.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("--print-prompt").or(predicate::str::contains("--apply")));
}

// ---------------------------------------------------------------------------
// Help / version still work post-subcommand split
// ---------------------------------------------------------------------------

#[test]
fn init_help_lists_documented_flags() {
    let mut cmd = cargo_bin_dirsql();
    cmd.arg("init").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--root"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--infer"));
}

#[test]
fn top_level_help_lists_init_subcommand() {
    let mut cmd = cargo_bin_dirsql();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("init"));
}
