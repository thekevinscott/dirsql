//! A live server applies a file edit that swaps a UNIQUE value between two
//! keyed rows, as a fresh build of the same file would. Real binary, real
//! filesystem, nothing mocked.

#![cfg(feature = "cli")]

#[path = "common/server.rs"]
mod server;

use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn free_port() -> u16 {
    TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_until_ready(port: u16) {
    let client = Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if client
            .get(format!("http://localhost:{port}/query"))
            .send()
            .is_ok()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("dirsql server did not become ready on port {port}");
}

/// Every row event the server streams, after its subscription is attached.
fn subscribe(port: u16) -> mpsc::Receiver<Value> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let resp = Client::builder()
            .timeout(None)
            .build()
            .unwrap()
            .get(format!("http://localhost:{port}/events"))
            .send()
            .unwrap();
        for line in BufReader::new(resp).lines().map_while(Result::ok) {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "{}" {
                ready_tx.send(()).ok();
                continue;
            }
            if tx.send(serde_json::from_str(data).unwrap()).is_err() {
                return;
            }
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("SSE stream never produced a ready sentinel");
    rx
}

// Staged outside the glob and renamed into place: `fs::write` truncates
// before it writes, and the watcher can read the file in between.
fn write_rows(root: &Path, body: &str) {
    let staged = root.join("d/a.tmp");
    fs::write(&staged, body).unwrap();
    fs::rename(&staged, root.join("d/a.jsonl")).unwrap();
}

#[test]
fn the_server_swaps_a_unique_value_between_keyed_rows() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"x\"}\n{\"id\":2,\"name\":\"y\"}\n",
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT UNIQUE)\"\nglob = \"d/*.jsonl\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let port = free_port();
    let _server = server::spawn_server(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port);
    let events = subscribe(port);
    std::thread::sleep(Duration::from_millis(300));

    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"y\"}\n{\"id\":2,\"name\":\"x\"}\n",
    );

    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && seen.len() < 2 {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(200)) {
            seen.push(event);
        }
    }
    let actions: Vec<&str> = seen.iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["update", "update"], "events: {seen:?}");

    let rows: Value = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT id, name FROM t ORDER BY id"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        rows,
        json!([{"id": 1, "name": "y"}, {"id": 2, "name": "x"}])
    );
}
