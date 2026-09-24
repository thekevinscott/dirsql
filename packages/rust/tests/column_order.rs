//! Integration tests for column order: a result carries the order the SELECT
//! list asked for, not the alphabetical order a `HashMap`/`BTreeMap` falls
//! into. Exercised through the `/query` transport, where the serialized key
//! order is observable.
//!
//! Gated behind `--features cli` — the server lives in `src/cli/`.

#![cfg(feature = "cli")]

use std::fs;

use dirsql::DirSQL;
use dirsql::cli::{ServerConfig, ServerHandle, serve};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.md"), "alpha\n").unwrap();
    root
}

async fn spawn(db: DirSQL) -> ServerHandle {
    serve(ServerConfig::ephemeral(), db)
        .await
        .expect("server should bind on an ephemeral port")
}

async fn query_body(handle: &ServerHandle, sql: &str) -> String {
    reqwest::Client::new()
        .post(format!("http://{}/query", handle.local_addr()))
        .json(&serde_json::json!({ "sql": sql }))
        .send()
        .await
        .expect("POST /query failed")
        .text()
        .await
        .expect("response body must be text")
}

fn key_positions(body: &str, keys: [&str; 2]) -> [usize; 2] {
    keys.map(|key| {
        body.find(&format!("\"{key}\""))
            .unwrap_or_else(|| panic!("`{key}` missing from response: {body}"))
    })
}

#[tokio::test]
async fn serialized_rows_follow_the_projection_order() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    let handle = spawn(db).await;

    let body = query_body(&handle, "SELECT size, basename FROM './'").await;
    let [size, basename] = key_positions(&body, ["size", "basename"]);

    assert!(
        size < basename,
        "expected `size` before `basename`, got: {body}"
    );
}

#[tokio::test]
async fn reversing_the_select_list_reverses_the_serialized_order() {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    let handle = spawn(db).await;

    let body = query_body(&handle, "SELECT basename, size FROM './'").await;
    let [size, basename] = key_positions(&body, ["size", "basename"]);

    assert!(
        basename < size,
        "expected `basename` before `size`, got: {body}"
    );
}
