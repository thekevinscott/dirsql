//! A long-lived `dirsql` process answers a query issued the moment a write
//! returns with that write: over the server's HTTP query endpoint, and in the
//! REPL. Real binary, real filesystem, nothing mocked.

#![cfg(all(feature = "cli", target_os = "linux"))]

#[path = "common/fresh.rs"]
mod fresh;
#[path = "common/server.rs"]
mod server;

use assert_cmd::prelude::*;
use fresh::Tree;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

const ROUNDS: usize = 300;
const SQL: &str = "SELECT v FROM recs WHERE k = 'h' ORDER BY v";

fn record(v: &str) -> String {
    format!("{{\"k\":\"h\",\"v\":\"{v}\"}}")
}

fn values(rows: &Value) -> Vec<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| row["v"].as_str().unwrap().to_string())
        .collect()
}

/// Create, edit and delete a file each round, asking `query` straight after
/// every write.
fn create_edit_delete(tree: &Tree, mut query: impl FnMut() -> Vec<String>) {
    for i in 0..ROUNDS {
        let rel = format!("h/{i}.json");
        tree.write(&rel, &record(&format!("c{i}")));
        assert_eq!(query(), [format!("c{i}")], "create {i}");
        tree.write(&rel, &record(&format!("e{i}")));
        assert_eq!(query(), [format!("e{i}")], "edit {i}");
        tree.remove(&rel);
        assert!(query().is_empty(), "delete {i}");
    }
}

#[test]
fn the_server_answers_every_query_with_the_write_before_it() {
    let tree = Tree::new();
    let port = TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = tree.path(".dirsql.toml");
    let mut server = server::spawn_server(
        tree.root(),
        port,
        &["--config".as_ref(), config.as_os_str()],
    );
    let mut banner = String::new();
    BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();
    assert!(banner.starts_with("Running at"), "{banner:?}");
    let client = Client::new();
    let url = format!("http://localhost:{port}/query");
    create_edit_delete(&tree, || {
        let rows: Value = client
            .post(&url)
            .json(&json!({ "sql": SQL }))
            .send()
            .unwrap()
            .json()
            .unwrap();
        values(&rows)
    });
}

#[test]
fn the_repl_answers_every_statement_with_the_write_before_it() {
    let tree = Tree::new();
    let mut repl = Command::cargo_bin("dirsql")
        .unwrap()
        .arg("--config")
        .arg(tree.path(".dirsql.toml"))
        .current_dir(tree.root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut stdin = repl.stdin.take().unwrap();
    let mut stdout = BufReader::new(repl.stdout.take().unwrap());
    create_edit_delete(&tree, || {
        writeln!(stdin, "{SQL}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        values(&serde_json::from_str(&line).unwrap())
    });
    drop(stdin);
    assert!(repl.wait().unwrap().success());
}
