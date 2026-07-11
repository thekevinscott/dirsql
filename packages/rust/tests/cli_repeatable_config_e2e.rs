//! E2E red tests for #547: the `--config`/`-c` flag is repeatable.
//!
//! Spawns the actual compiled `dirsql` binary; nothing mocked. Today clap
//! declares `--config` single-valued, so a second `-c` is rejected before the
//! server (or `query`) ever starts — every test here fails on the resulting
//! startup failure / non-zero exit.
//!
//! Gated behind `--features cli` like the sibling `cli_e2e.rs`.
#![cfg(feature = "cli")]

use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::prelude::*;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use tempfile::TempDir;

/// Write `.dirsql.toml` declaring one single-column table into `dir`.
fn write_table_config(dir: &Path, table: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(
        &path,
        format!(
            r#"
[[table]]
ddl = "CREATE TABLE {table} (basename TEXT)"
glob = "*.json"
"#
        ),
    )
    .unwrap();
    path
}

fn free_port() -> u16 {
    TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Wait for the server to answer HTTP, failing fast (with the reason) if the
/// child exits first — which is exactly what happens while `-c` is not yet
/// repeatable: clap rejects the duplicate flag and the process dies.
fn wait_until_ready_or_exit(child: &mut Child, port: u16, timeout: Duration) {
    let client = Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
    let url = format!("http://localhost:{port}/query");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            panic!(
                "dirsql must accept repeated --config/-c flags, but exited at startup: {status}"
            );
        }
        if client.get(&url).send().is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("dirsql server did not become ready on port {port} within {timeout:?}");
}

#[test]
fn server_serves_tables_from_two_config_flags() {
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();
    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_table_config(cfg_a.path(), "alpha");
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_table_config(cfg_b.path(), "beta");

    let port = free_port();
    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .arg("-c")
        .arg(&cfg_a_path)
        .arg("-c")
        .arg(&cfg_b_path)
        .current_dir(data.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = cmd.spawn().expect("spawning dirsql failed");

    wait_until_ready_or_exit(&mut child, port, Duration::from_secs(10));

    let client = Client::new();
    for table in ["alpha", "beta"] {
        let resp = client
            .post(format!("http://localhost:{port}/query"))
            .json(&json!({"sql": format!("SELECT COUNT(*) AS n FROM {table}")}))
            .send()
            .unwrap();
        assert_eq!(
            resp.status(),
            reqwest::StatusCode::OK,
            "table {table} from one of the two -c configs must be served"
        );
        let body: Vec<Value> = resp.json().unwrap();
        assert_eq!(body, vec![json!({"n": 1})], "table {table} must be indexed");
    }

    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn query_subcommand_accepts_repeated_config_flags() {
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();
    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_table_config(cfg_a.path(), "alpha");
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_table_config(cfg_b.path(), "beta");

    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli");
    let out = cmd
        .arg("--config")
        .arg(&cfg_a_path)
        .arg("--config")
        .arg(&cfg_b_path)
        .arg("query")
        .arg("SELECT COUNT(*) AS n FROM alpha")
        .current_dir(data.path())
        .output()
        .expect("spawning `dirsql query` failed");

    assert!(
        out.status.success(),
        "`dirsql query` must accept repeated --config flags, got status {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).expect("stdout must be rows JSON");
    assert_eq!(rows, vec![json!({"n": 1})]);
}
