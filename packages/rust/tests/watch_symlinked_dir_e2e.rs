//! A live server drops the rows reached through a symlinked directory once
//! the link's target leaves the tree, as a fresh scan would. Real binary,
//! real filesystem, nothing mocked.

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

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

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

#[test]
fn the_server_deletes_rows_reached_through_a_link_whose_target_left_the_tree() {
    let root = TempDir::new().unwrap();
    let away = TempDir::new().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/x.json"), "{\"n\":1}\n").unwrap();
    symlink_dir(&root.path().join("real"), &root.path().join("link"));
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (n INTEGER)\"\nglob = \"*/*.json\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let port = free_port();
    let _server = server::spawn_server(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port);
    let events = subscribe(port);
    std::thread::sleep(Duration::from_millis(300));

    fs::rename(root.path().join("real"), away.path().join("real")).unwrap();

    let mut seen = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let Ok(event) = events.recv_timeout(Duration::from_millis(200)) else {
            if seen
                .iter()
                .filter(|e: &&Value| e["action"] == "delete")
                .count()
                >= 2
            {
                break;
            }
            continue;
        };
        seen.push(event);
    }
    let actions: Vec<&str> = seen.iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["delete", "delete"], "events: {seen:?}");

    let rows: Value = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT n FROM t"}))
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(rows, json!([]));
}
