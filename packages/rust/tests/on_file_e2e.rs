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

use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::prelude::*;
use reqwest::{StatusCode, blocking::Client};
use serde_json::{Value, json};
use tempfile::TempDir;

fn free_port() -> u16 {
    TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn spawn_dirsql(dir: &std::path::Path, port: u16) -> Child {
    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built with --features cli");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    cmd.spawn().expect("spawning dirsql failed")
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

fn kill_and_wait(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn on_file_rows_are_served_over_http() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob = "**/meta.json"
on-file = "cat {path}"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("p1")).unwrap();
    fs::write(
        root.path().join("p1").join("meta.json"),
        r#"[{"paper_id":"a","title":"First"},{"paper_id":"b","title":"Second"}]"#,
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

#[test]
fn on_file_abspath_token_is_no_longer_substituted() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("echo_args.sh"),
        "#!/bin/sh\nprintf '[{\"q\":\"%s\"}]' \"$2\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (q TEXT)"
glob = "*.json"
on-file = "sh echo_args.sh {path} {abspath}"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.json"), "ignored\n").unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    // The helper echoes its second arg (the `{abspath}` slot) into `q`. Since
    // `{abspath}` is no longer substituted, it arrives as the literal string.
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
fn a_file_whose_command_errors_is_skipped_while_the_rest_succeed() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("extract.sh"),
        "#!/bin/sh\nif grep -q BOOM \"$1\"; then exit 1; fi\nprintf '[{\"name\":\"ok\"}]'\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh extract.sh {path}"
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
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"name": "ok"})]);

    kill_and_wait(child);
}
