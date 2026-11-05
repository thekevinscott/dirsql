//! CLI e2e for the repo-wide exclude files: the real `dirsql` binary over a
//! real git repo, nothing mocked. Pins that `$GIT_DIR/info/exclude` and the
//! `core.excludesFile` global ignore hide files as git, fd and ripgrep do.

#![cfg(feature = "cli")]

use std::fs;
use std::path::Path;
use std::process::Output;

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

/// A repo holding `a.txt` and `b.txt`.
fn repo() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".git/info")).unwrap();
    fs::write(root.path().join("a.txt"), "").unwrap();
    fs::write(root.path().join("b.txt"), "").unwrap();
    root
}

/// A git config environment rooted in `home`: no system config, and a
/// global config naming `excludes` as `core.excludesFile` when given.
fn git_env(home: &Path, excludes: Option<&Path>) -> Vec<(String, String)> {
    let global = home.join("gitconfig");
    let body = excludes.map_or_else(String::new, |file| {
        let file = file.to_string_lossy().replace('\\', "/");
        format!("[core]\n\texcludesFile = {file}\n")
    });
    fs::write(&global, body).unwrap();
    let system = home.join("system-gitconfig");
    fs::write(&system, "").unwrap();
    let text = |path: &Path| path.to_string_lossy().into_owned();
    vec![
        ("HOME".into(), text(home)),
        ("USERPROFILE".into(), text(home)),
        ("XDG_CONFIG_HOME".into(), text(&home.join("xdg"))),
        ("GIT_CONFIG_GLOBAL".into(), text(&global)),
        ("GIT_CONFIG_SYSTEM".into(), text(&system)),
    ]
}

fn run(dir: &Path, env: &[(String, String)], args: &[&str]) -> Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(dir)
        .output()
        .expect("spawning `dirsql` failed")
}

fn paths(out: &Output) -> Vec<String> {
    assert!(
        out.status.success(),
        "expected success, stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> =
        serde_json::from_slice(&out.stdout).expect("stdout must be a JSON array");
    rows.into_iter()
        .map(|r| r["path"].as_str().unwrap().to_string())
        .collect()
}

const QUERY: &str = "SELECT path FROM './*' ORDER BY path";

#[test]
fn info_exclude_hides_the_files_it_names() {
    let root = repo();
    let home = TempDir::new().unwrap();
    fs::write(root.path().join(".git/info/exclude"), "a.txt\n").unwrap();

    let out = run(root.path(), &git_env(home.path(), None), &["query", QUERY]);

    assert_eq!(paths(&out), vec!["b.txt"]);
}

#[test]
fn core_excludes_file_hides_the_files_it_names() {
    let root = repo();
    let home = TempDir::new().unwrap();
    let excludes = home.path().join("global-ignore");
    fs::write(&excludes, "b.txt\n").unwrap();

    let out = run(
        root.path(),
        &git_env(home.path(), Some(&excludes)),
        &["query", QUERY],
    );

    assert_eq!(paths(&out), vec!["a.txt"]);
}

#[test]
fn the_default_global_ignore_applies_when_core_excludes_file_is_unset() {
    let root = repo();
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join("xdg/git")).unwrap();
    fs::write(home.path().join("xdg/git/ignore"), "a.txt\n").unwrap();

    let out = run(root.path(), &git_env(home.path(), None), &["query", QUERY]);

    assert_eq!(paths(&out), vec!["b.txt"]);
}

#[test]
fn no_ignore_restores_files_the_exclude_files_hide() {
    let root = repo();
    let home = TempDir::new().unwrap();
    let excludes = home.path().join("global-ignore");
    fs::write(&excludes, "b.txt\n").unwrap();
    fs::write(root.path().join(".git/info/exclude"), "a.txt\n").unwrap();

    let out = run(
        root.path(),
        &git_env(home.path(), Some(&excludes)),
        &["query", QUERY, "--no-ignore"],
    );

    assert_eq!(paths(&out), vec!["a.txt", "b.txt"]);
}
