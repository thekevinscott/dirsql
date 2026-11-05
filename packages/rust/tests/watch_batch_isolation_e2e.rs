//! A server query issued while a watch batch runs never sees it half applied,
//! nor a row the batch's foreign keys roll back. Real binary, real
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
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const SLOW_CAT: &str = r#"on-file = """sh -c 'sleep 1; cat "$@"' sh""""#;

fn serve(root: &Path) -> (server::ServerGuard, String) {
    let port = TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = server::spawn_server(root, port, &["-c", ".dirsql.toml"]);
    let mut banner = String::new();
    BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();
    assert!(banner.starts_with("Running at"), "{banner:?}");
    (server, format!("http://localhost:{port}/query"))
}

fn ask(client: &Client, url: &str, sql: &str) -> Value {
    client
        .post(url)
        .json(&json!({ "sql": sql }))
        .send()
        .unwrap()
        .json()
        .unwrap()
}

#[test]
fn every_query_sees_both_tables_fed_by_one_batch_at_the_same_point() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("d")).unwrap();
    let table = |name: &str| {
        format!(
            "[[table]]\nname = \"{name}\"\nddl = \"CREATE TABLE {name} (line TEXT)\"\nglob = \"d/*.jsonl\"\n{SLOW_CAT}\n"
        )
    };
    fs::write(
        root.join(".dirsql.toml"),
        format!("{}\n{}", table("a"), table("b")),
    )
    .unwrap();
    fs::write(root.join("d").join("1.jsonl"), "{\"line\":\"one\"}\n").unwrap();
    let (_server, url) = serve(root);
    let client = Client::new();

    fs::write(root.join("d").join("2.jsonl"), "{\"line\":\"two\"}\n").unwrap();
    let mut seen = Vec::new();
    for _ in 0..25 {
        seen.push(ask(
            &client,
            &url,
            "SELECT (SELECT count(*) FROM a) AS na, (SELECT count(*) FROM b) AS nb",
        ));
        thread::sleep(Duration::from_millis(100));
    }

    for rows in &seen {
        assert_eq!(rows[0]["na"], rows[0]["nb"], "torn read in {seen:?}");
    }
    assert_eq!(seen.last().unwrap()[0]["na"], 2, "{seen:?}");
}

#[test]
fn no_query_sees_a_row_from_a_batch_its_foreign_keys_reject() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("c")).unwrap();
    fs::write(
        root.join(".dirsql.toml"),
        r#"[[table]]
name = "child"
ddl = "CREATE TABLE child (id TEXT PRIMARY KEY, parent TEXT REFERENCES parent(k))"
glob = "c/*.jsonl"
on-file = "cat"

[[table]]
name = "parent"
ddl = "CREATE TABLE parent (k TEXT PRIMARY KEY)"
glob = "c/*.jsonl"
on-file = """sh -c 'sleep 2; cat "$0"' p.jsonl"""
"#,
    )
    .unwrap();
    fs::write(root.join("p.jsonl"), "{\"k\":\"p1\"}\n").unwrap();
    fs::write(
        root.join("c").join("a.jsonl"),
        "{\"id\":\"c1\",\"parent\":\"p1\"}\n",
    )
    .unwrap();
    let (_server, url) = serve(root);
    let client = Client::new();

    fs::write(
        root.join("c").join("a.jsonl"),
        "{\"id\":\"c1\",\"parent\":\"p1\"}\n{\"id\":\"c2\",\"parent\":\"MISSING\"}\n",
    )
    .unwrap();
    let mut seen = Vec::new();
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(400));
        seen.push(ask(&client, &url, "SELECT id FROM child ORDER BY id"));
    }

    for answer in &seen {
        let ids: Vec<&Value> = answer
            .as_array()
            .map(|rows| rows.iter().map(|row| &row["id"]).collect())
            .unwrap_or_default();
        assert!(!ids.contains(&&json!("c2")), "dirty read in {seen:?}");
    }
}
