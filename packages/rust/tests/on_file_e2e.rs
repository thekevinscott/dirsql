//! End-to-end tests for the `on-file` per-table command event.
//!
//! These spawn the real compiled `dirsql` binary over a temp directory whose
//! `.dirsql.toml` declares an `on-file` command, talk to it over real HTTP,
//! and assert the produced rows. Nothing is mocked (real process, real
//! filesystem, real SQLite, real command spawn).
//!
//! Gated behind `--features cli` (the `dirsql` bin needs it) and Unix (the
//! fixtures shell out to `sh`/`cat`); the Rust CI test job runs on Linux.

#![cfg(all(feature = "cli", unix))]

#[path = "common/server.rs"]
mod server;

use std::fs;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use reqwest::{StatusCode, blocking::Client};
use serde_json::{Value, json};
use server::ServerGuard;
use tempfile::TempDir;

fn free_port() -> u16 {
    TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn spawn_dirsql(dir: &std::path::Path, port: u16) -> ServerGuard {
    // Bare `dirsql` no longer auto-loads a cwd `.dirsql.toml` (#602); these
    // fixtures place their config at the index root, so pass it explicitly.
    let config = dir.join(".dirsql.toml");
    spawn_dirsql_with(dir, Some(&config), port)
}

/// Spawn `dirsql` from working directory `cwd`, optionally pointing `--config`
/// at a config file elsewhere (so the config dir can differ from the cwd).
fn spawn_dirsql_with(
    cwd: &std::path::Path,
    config: Option<&std::path::Path>,
    port: u16,
) -> ServerGuard {
    match config {
        Some(config) => server::spawn_server(cwd, port, &["--config".as_ref(), config.as_os_str()]),
        None => server::spawn_server::<&str>(cwd, port, &[]),
    }
}

fn wait_until_ready(port: u16, timeout: Duration) {
    let client = Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
    let url = format!("http://localhost:{port}/query");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if client.get(&url).send().is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("dirsql server did not become ready on port {port} within {timeout:?}");
}

fn kill_and_wait(child: ServerGuard) {
    drop(child);
}

#[test]
fn on_file_rows_are_served_over_http() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "papers"
ddl = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob = "**/meta.json"
on-file = "cat"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("p1")).unwrap();
    fs::write(
        root.path().join("p1").join("meta.json"),
        "{\"paper_id\":\"a\",\"title\":\"First\"}\n{\"paper_id\":\"b\",\"title\":\"Second\"}",
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT paper_id, title FROM papers ORDER BY paper_id"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![
            json!({"paper_id": "a", "title": "First"}),
            json!({"paper_id": "b", "title": "Second"}),
        ]
    );

    kill_and_wait(child);
}

/// Each appended path is the matched file's **absolute** path. The `on-file`
/// script exits non-zero unless its argument is absolute (`case $1 in /*)`)
/// and then `cat`s it; rows arriving over HTTP prove the script received an
/// absolute path.
#[test]
fn on_file_receives_absolute_path_over_http() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("abscheck.sh"),
        "#!/bin/sh\ncase \"$1\" in /*) cat \"$1\" ;; *) exit 1 ;; esac\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "papers"
ddl = "CREATE TABLE papers (paper_id TEXT)"
glob = "**/meta.json"
on-file = "sh abscheck.sh"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("p1")).unwrap();
    fs::write(
        root.path().join("p1").join("meta.json"),
        r#"{"paper_id":"a"}"#,
    )
    .unwrap();

    let port = free_port();
    let _server = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT paper_id FROM papers"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"paper_id": "a"})]);
}

/// The absolute path resolves even when the hook's working directory (the
/// config dir) is not the index root. Since #540 the index root is the
/// invocation cwd, so `dirsql` is launched from another dir while its config
/// lives elsewhere, reached via an absolute `--config`, and the glob anchors at
/// the config dir. The hook (cwd = config dir) `cat`s the file only because the
/// path is absolute.
#[test]
fn on_file_absolute_path_resolves_when_config_dir_differs_from_root() {
    let project = TempDir::new().unwrap();
    let configdir = TempDir::new().unwrap();
    fs::write(
        configdir.path().join("abscheck.sh"),
        "#!/bin/sh\ncase \"$1\" in /*) cat \"$1\" ;; *) exit 1 ;; esac\n",
    )
    .unwrap();
    fs::write(
        configdir.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "papers"
ddl = "CREATE TABLE papers (paper_id TEXT)"
glob = "**/meta.json"
on-file = "sh abscheck.sh"
"#,
    )
    .unwrap();
    fs::create_dir_all(configdir.path().join("data")).unwrap();
    fs::write(
        configdir.path().join("data").join("meta.json"),
        r#"{"paper_id":"a"}"#,
    )
    .unwrap();

    let port = free_port();
    let _server = spawn_dirsql_with(
        project.path(),
        Some(&configdir.path().join(".dirsql.toml")),
        port,
    );
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT paper_id FROM papers"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"paper_id": "a"})]);
}

#[test]
fn on_file_abspath_token_is_not_substituted() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("echo_args.sh"),
        "#!/bin/sh\nprintf '{\"q\":\"%s\"}' \"$1\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (q TEXT)"
glob = "*.json"
on-file = "sh echo_args.sh {abspath}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), "ignored\n").unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    // The helper echoes its first arg (the `{abspath}` slot) into `q`. Since
    // `{abspath}` is not substituted, it arrives as the literal string.
    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT q FROM items"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"q": "{abspath}"})]);

    kill_and_wait(child);
}

#[test]
fn on_file_runs_once_with_every_matched_path_appended() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("count.sh"),
        "#!/bin/sh\nprintf '{\"n\":%s}' \"$#\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (n INTEGER)"
glob = "*.json"
on-file = "sh count.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), "x").unwrap();
    fs::write(root.path().join("b.json"), "y").unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT n FROM items"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"n": 2})]);

    kill_and_wait(child);
}

#[test]
fn a_table_whose_command_errors_leaves_the_server_unavailable() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$@\"; then exit 1; fi\nprintf '{\"name\":\"ok\"}'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh"
"#,
    )
    .unwrap();
    fs::write(root.path().join("good.txt"), "fine\n").unwrap();
    fs::write(root.path().join("bad.txt"), "BOOM\n").unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT name FROM items"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: Value = resp.json().unwrap();
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("`items`"),
        "the failed table is named in the error: {body}"
    );

    kill_and_wait(child);
}
