//! End-to-end: `dirsql server` streams no event for a file rewritten with
//! rows the table already holds, and reports a new row as `POST /query`
//! returns it. Nothing is mocked.
//!
//! Gated behind `--features cli`: the `dirsql` bin target is
//! `required-features = ["cli"]`.

#![cfg(feature = "cli")]

#[path = "common/server.rs"]
mod server;

use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde_json::{Value, json};
use tempfile::TempDir;

const ROW: &str = "{\"k\":\"a\",\"x\":1,\"s\":5,\"i\":\"7\"}\n";

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

/// Subscribe to `/events`, returning once the `ready` sentinel arrives; each
/// later event's JSON is sent on the returned channel.
fn subscribe(port: u16) -> mpsc::Receiver<Value> {
    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel::<Value>();
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
            let data: Value = serde_json::from_str(data.trim()).unwrap();
            if data == json!({}) {
                ready_tx.send(()).ok();
                continue;
            }
            if tx.send(data).is_err() {
                break;
            }
        }
    });
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("SSE stream never produced a ready sentinel");
    rx
}

#[test]
fn a_rewrite_with_stored_rows_unchanged_streams_nothing_and_a_new_row_streams_as_stored() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("d")).unwrap();
    fs::write(root.path().join("d/a.jsonl"), ROW).unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (k TEXT PRIMARY KEY, x REAL, s TEXT, i INTEGER)\"\nglob = \"d/*.jsonl\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let port = free_port();
    let _server = server::spawn_server(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port);
    let events = subscribe(port);
    std::thread::sleep(Duration::from_millis(200));

    fs::write(root.path().join("d/a.jsonl"), ROW).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    fs::write(
        root.path().join("d/a.jsonl"),
        format!("{ROW}{}", "{\"k\":\"b\",\"x\":2,\"s\":6,\"i\":\"8\"}\n"),
    )
    .unwrap();

    let mut seen = vec![
        events
            .recv_timeout(Duration::from_secs(10))
            .expect("no event for the new row"),
    ];
    while let Ok(event) = events.recv_timeout(Duration::from_secs(1)) {
        seen.push(event);
    }
    let rows: Vec<Value> = seen
        .iter()
        .map(|event| json!({"action": event["action"], "row": event["row"]}))
        .collect();
    assert_eq!(
        rows,
        vec![json!({"action": "insert", "row": {"k": "b", "x": 2.0, "s": "6", "i": 8}})],
        "events: {seen:?}"
    );
}
