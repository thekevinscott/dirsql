//! Integration tests for the `dirsql` CLI HTTP server (issue #105).
//!
//! These tests exercise the server in-process against a real `DirSQL`
//! instance: no subprocess, no filesystem beyond the fixture tempdir.
//! Third-party HTTP transport is real (`reqwest`, `eventsource-client`);
//! everything below the `dirsql::cli` module is live.
//!
//! Gated behind `--features cli` — the module under test lives in
//! `src/cli/`, which is only compiled when that feature is on. Runs
//! clean under `cargo test -p dirsql --features cli`; compiled to an
//! empty test binary otherwise so `cargo test` (no features) and
//! `cargo llvm-cov` without the flag still succeed.

#![cfg(feature = "cli")]

use std::time::Duration;

use dirsql::DirSQL;
use dirsql::cli::{ServerConfig, ServerHandle, serve};
use eventsource_client::{Client, SSE};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{Value as JsonValue, json};
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a `DirSQL` over a two-post blog fixture driven by `.dirsql.toml`,
/// matching the e2e fixture shape. Returns the tempdir so the caller can
/// mutate files while the server runs.
fn blog_fixture() -> (TempDir, DirSQL) {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::write(
        root.path().join("posts/hello.json"),
        r#"{"title":"Hello World","author":"alice"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("posts/second.json"),
        r#"{"title":"Second Post","author":"bob"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.json"
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    (root, db)
}

/// Bind the server on an ephemeral port and return the live handle.
async fn spawn_server(db: DirSQL) -> ServerHandle {
    serve(ServerConfig::ephemeral(), db)
        .await
        .expect("server should bind on an ephemeral port")
}

fn base_url(handle: &ServerHandle) -> String {
    format!("http://{}", handle.local_addr())
}

/// Drive the SSE stream until the server-emitted `ready` sentinel arrives.
/// This primes the underlying HTTP connection so subsequent subscriptions
/// don't miss events fired immediately after.
async fn await_ready<S>(stream: &mut S)
where
    S: futures_util::Stream<Item = Result<SSE, eventsource_client::Error>> + Unpin,
{
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(evt) = stream.next().await {
            if let Ok(SSE::Event(ev)) = evt {
                if ev.event_type == "ready" {
                    return;
                }
            }
        }
        panic!("stream closed before ready sentinel arrived");
    })
    .await
    .expect("timed out waiting for SSE `ready` sentinel");
}

async fn await_row_event<S>(stream: &mut S, timeout: std::time::Duration) -> JsonValue
where
    S: futures_util::Stream<Item = Result<SSE, eventsource_client::Error>> + Unpin,
{
    tokio::time::timeout(timeout, async {
        while let Some(evt) = stream.next().await {
            let Ok(SSE::Event(ev)) = evt else { continue };
            if ev.event_type == "ready" {
                continue;
            }
            return serde_json::from_str(&ev.data).unwrap();
        }
        panic!("stream closed before row event arrived");
    })
    .await
    .expect("timed out waiting for SSE row event")
}

// ---------------------------------------------------------------------------
// POST /query
// ---------------------------------------------------------------------------

#[tokio::test]
async fn post_query_returns_json_rows_on_success() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": "SELECT title FROM posts ORDER BY title"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<JsonValue> = resp.json().await.unwrap();
    assert_eq!(
        body,
        vec![
            json!({"title": "Hello World"}),
            json!({"title": "Second Post"}),
        ]
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn post_query_missing_sql_field_returns_400() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: JsonValue = resp.json().await.unwrap();
    assert!(
        body.get("error").is_some(),
        "400 body should carry a JSON `error` field, got {body}"
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn post_query_empty_sql_returns_400() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": ""}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn post_query_malformed_sql_returns_400_not_500() {
    // The client sent bad input; server misuse shouldn't masquerade as 5xx.
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": "SLECT * FORM posts"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn post_query_non_json_body_returns_400() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .body("this is not JSON")
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    handle.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Method mismatches
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_query_returns_405() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/query", base_url(&handle)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn post_events_returns_405() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/events", base_url(&handle)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    handle.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// GET /events (SSE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_events_streams_mutation_events() {
    let (root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let client =
        eventsource_client::ClientBuilder::for_url(&format!("{}/events", base_url(&handle)))
            .unwrap()
            .build();

    let mut stream = client.stream();

    // Await the server's "ready" sentinel so we know the subscription is
    // attached before we mutate. Without this, eventsource-client connects
    // lazily on first poll and the mutation can fire before the subscriber
    // exists.
    await_ready(&mut stream).await;

    fs::write(
        root.path().join("posts/hello.json"),
        r#"{"title":"Hello, world","author":"alice"}"#,
    )
    .unwrap();

    let payload = await_row_event(&mut stream, Duration::from_secs(5)).await;
    assert_eq!(
        payload.get("action").and_then(JsonValue::as_str),
        Some("update")
    );
    assert_eq!(
        payload.get("table").and_then(JsonValue::as_str),
        Some("posts")
    );

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn get_events_surfaces_parse_errors_as_error_events_not_fatal() {
    // Per docs/guide/cli.md: an error during extraction is a per-event problem,
    // not a server-wide one. The stream must keep delivering subsequent events.
    let (root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let client =
        eventsource_client::ClientBuilder::for_url(&format!("{}/events", base_url(&handle)))
            .unwrap()
            .build();
    let mut stream = client.stream();

    await_ready(&mut stream).await;

    // Break a file — extract should fail on this one.
    fs::write(root.path().join("posts/hello.json"), "not valid json").unwrap();
    // Then fix another file to produce a valid event.
    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::write(
        root.path().join("posts/second.json"),
        r#"{"title":"Second Post v2","author":"bob"}"#,
    )
    .unwrap();

    let mut saw_error = false;
    let mut saw_normal_after_error = false;

    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(evt) = stream.next().await {
            let Ok(SSE::Event(ev)) = evt else { continue };
            if ev.event_type == "ready" {
                continue;
            }
            let payload: JsonValue = serde_json::from_str(&ev.data).unwrap();
            match payload.get("action").and_then(JsonValue::as_str) {
                Some("error") => saw_error = true,
                Some(_) if saw_error => {
                    saw_normal_after_error = true;
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert!(saw_error, "expected an `error` event for malformed file");
    assert!(
        saw_normal_after_error,
        "expected the stream to keep delivering events after the error"
    );

    handle.shutdown().await.unwrap();
}

// ---------------------------------------------------------------------------
// Graceful shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_drains_in_flight_requests() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;
    let url = format!("{}/query", base_url(&handle));

    // A recursive CTE that SQLite takes a measurable fraction of a second
    // to evaluate. That gives us a deterministic window during which the
    // request is guaranteed to be in-flight inside the handler — no more
    // "hope the sleep is long enough" race with `SELECT 1`.
    let slow_sql = "WITH RECURSIVE c(x) AS (\
        SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 500000\
    ) SELECT COUNT(*) AS n FROM c";

    let req = tokio::spawn({
        let url = url.clone();
        let sql = slow_sql.to_string();
        async move {
            reqwest::Client::new()
                .post(&url)
                .json(&json!({ "sql": sql }))
                .send()
                .await
        }
    });

    // Give the spawned task time to send the request and for the handler
    // to begin evaluating the slow SQL. The slow-CTE window is hundreds
    // of ms, so 50ms of scheduler grace is plenty.
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown().await.unwrap();

    let resp = req
        .await
        .unwrap()
        .expect("in-flight request should not be cut off");
    assert!(resp.status().is_success());

    // After shutdown, a fresh request must fail to connect.
    let after = reqwest::Client::new()
        .post(&url)
        .json(&json!({"sql": "SELECT 1"}))
        .send()
        .await;
    assert!(after.is_err(), "post-shutdown requests should not connect");
}

// ---------------------------------------------------------------------------
// Bind / configuration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ephemeral_bind_picks_free_port_and_reports_it() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;
    let addr = handle.local_addr();
    assert_ne!(addr.port(), 0, "ephemeral bind must resolve to a real port");
    handle.shutdown().await.unwrap();
}
