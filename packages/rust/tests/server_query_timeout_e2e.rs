//! `dirsql server`'s per-query timeout bounds one query: once a query is
//! answered `408`, the next query runs at once instead of waiting on it.

#![cfg(feature = "cli")]

#[path = "common/server.rs"]
mod server;

use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use reqwest::{StatusCode, blocking::Client};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn query_after_a_timed_out_query_answers_at_once() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "a\n").unwrap();
    let port = TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = server::spawn_server::<&str>(root.path(), port, &[]);
    let mut banner = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut banner)
        .unwrap();

    let client = Client::builder().timeout(None).build().unwrap();
    let url = format!("http://localhost:{port}/query");
    let endless_sql = "WITH RECURSIVE r(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM r LIMIT 100000) \
        SELECT count(*) AS n FROM r a, r b";
    let resp = client
        .post(&url)
        .json(&json!({ "sql": endless_sql }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);

    let started = Instant::now();
    let resp = client
        .post(&url)
        .json(&json!({ "sql": "SELECT path FROM './*'" }))
        .send()
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        elapsed < Duration::from_secs(5),
        "the query after a 408 waited {elapsed:?} on the timed-out one"
    );
}
