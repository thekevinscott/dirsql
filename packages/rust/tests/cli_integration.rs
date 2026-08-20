//! Integration tests for the `dirsql` CLI HTTP server.
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
use dirsql::cli::{AppState, ServerConfig, ServerError, ServerHandle, serve, serve_with_state};
use eventsource_client::{Client, SSE};
use futures_util::StreamExt;
use reqwest::StatusCode;
use serde_json::{Value as JsonValue, json};
use std::fs;
use tempfile::TempDir;

/// Build a `DirSQL` over a two-post blog fixture driven by `.dirsql.toml`.
/// Returns the tempdir so the caller can mutate files while the server runs.
///
/// Rows are identified by `basename`, a filesystem-derived column; `size` is
/// included so content-only edits still change a column value and surface as
/// `Update` events in the SSE stream.
fn blog_fixture() -> (TempDir, DirSQL) {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts/alice")).unwrap();
    fs::create_dir_all(root.path().join("posts/bob")).unwrap();
    fs::write(root.path().join("posts/alice/Hello-World.json"), "{}").unwrap();
    fs::write(root.path().join("posts/bob/Second-Post.json"), "{}").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "posts"
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
on-file = '''sh -c 'base=${1##*/}; size=$(wc -c < "$1" | tr -d " "); printf "[{\"basename\":\"%s\",\"size\":%s}]" "$base" "$size"' sh {path}'''
"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
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
            if let Ok(SSE::Event(ev)) = evt
                && ev.event_type == "ready"
            {
                return;
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

/// `From<DirSQL> for AppState` produces the ready arm. Lives here rather than
/// in `cli/mod.rs`'s inline unit module because populating the scanned
/// directory needs effectful `std::fs::write`, which the unit-lint isolation
/// rule bars from a unit test.
#[test]
fn from_dirsql_yields_ready_state() {
    let (_root, db) = blog_fixture();
    let state: AppState = db.into();
    assert!(
        matches!(state, AppState::Ready(_)),
        "From<DirSQL> must produce AppState::Ready",
    );
}

#[tokio::test]
async fn post_query_returns_json_rows_on_success() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<JsonValue> = resp.json().await.unwrap();
    assert_eq!(
        body,
        vec![
            json!({"basename": "Hello-World.json"}),
            json!({"basename": "Second-Post.json"}),
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

    // Modify the file's content; `size` is part of the row, so the diff
    // produces an Update event even though no captured path changed.
    fs::write(
        root.path().join("posts/alice/Hello-World.json"),
        r#"{"some":"larger","payload":"to change size"}"#,
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
    // An ingestion error is per-event, not fatal — the stream must keep
    // delivering. This table's extract parses JSON, so a malformed file
    // yields a per-file error event.
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::write(root.path().join("posts/first.json"), r#"{"ok":1}"#).unwrap();
    fs::write(root.path().join("posts/second.json"), r#"{"ok":2}"#).unwrap();

    let table = dirsql::Table::try_new(
        "posts",
        "CREATE TABLE posts (ok INTEGER, basename TEXT)",
        "posts/*.json",
        |path| {
            let content = std::fs::read_to_string(path)?;
            let value: JsonValue = serde_json::from_str(&content)?;
            let ok = value.get("ok").and_then(JsonValue::as_i64).unwrap_or(0);
            Ok(vec![std::collections::HashMap::from([(
                "ok".to_string(),
                dirsql::Value::Integer(ok),
            )])])
        },
    );
    let db = DirSQL::new(root.path(), vec![table]).unwrap();
    let handle = spawn_server(db).await;

    let client =
        eventsource_client::ClientBuilder::for_url(&format!("{}/events", base_url(&handle)))
            .unwrap()
            .build();
    let mut stream = client.stream();

    await_ready(&mut stream).await;

    fs::write(root.path().join("posts/first.json"), "{not valid json").unwrap();
    // Mutate another file to produce a valid event after the error.
    tokio::time::sleep(Duration::from_millis(50)).await;
    fs::write(root.path().join("posts/second.json"), r#"{"ok":99}"#).unwrap();

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

#[tokio::test]
async fn shutdown_drains_in_flight_requests() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;
    let url = format!("{}/query", base_url(&handle));

    // A recursive CTE slow enough to guarantee the request is still
    // in-flight inside the handler when shutdown fires.
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

    // Give the spawned task time to send the request; the slow-CTE window is
    // hundreds of ms, so 50ms of scheduler grace is plenty.
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.shutdown().await.unwrap();

    let resp = req
        .await
        .unwrap()
        .expect("in-flight request should not be cut off");
    assert!(resp.status().is_success());

    let after = reqwest::Client::new()
        .post(&url)
        .json(&json!({"sql": "SELECT 1"}))
        .send()
        .await;
    assert!(after.is_err(), "post-shutdown requests should not connect");
}

#[tokio::test]
async fn ephemeral_bind_picks_free_port_and_reports_it() {
    let (_root, db) = blog_fixture();
    let handle = spawn_server(db).await;
    let addr = handle.local_addr();
    assert_ne!(addr.port(), 0, "ephemeral bind must resolve to a real port");
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn query_in_unavailable_state_returns_503() {
    let handle = serve_with_state(
        ServerConfig::ephemeral(),
        AppState::Unavailable("config failed to load".into()),
    )
    .await
    .expect("server should bind even in the degraded state");

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": "SELECT 1"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: JsonValue = resp.json().await.unwrap();
    assert_eq!(
        body.get("error").and_then(JsonValue::as_str),
        Some("config failed to load"),
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn events_in_unavailable_state_returns_503() {
    let handle = serve_with_state(
        ServerConfig::ephemeral(),
        AppState::Unavailable("no config".into()),
    )
    .await
    .expect("server should bind even in the degraded state");

    let resp = reqwest::Client::new()
        .get(format!("{}/events", base_url(&handle)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn slow_query_exceeding_timeout_returns_408() {
    let (_root, db) = blog_fixture();
    let config = ServerConfig::ephemeral().with_query_timeout(Duration::from_millis(1));
    let handle = serve(config, db)
        .await
        .expect("server should bind on an ephemeral port");

    let slow_sql = "WITH RECURSIVE c(x) AS (\
        SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 5000000\
    ) SELECT COUNT(*) AS n FROM c";

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({ "sql": slow_sql }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::REQUEST_TIMEOUT);
    let body: JsonValue = resp.json().await.unwrap();
    assert!(
        body.get("error")
            .and_then(JsonValue::as_str)
            .is_some_and(|m| m.contains("timeout")),
        "408 body should describe the timeout, got {body}",
    );
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn binding_an_in_use_port_surfaces_bind_error() {
    let (_root, db) = blog_fixture();
    let first = spawn_server(db).await;
    let addr = first.local_addr();
    let port = addr.port();

    // Re-bind the *exact* address the first server resolved to (localhost may
    // map to 127.0.0.1 or ::1), so the conflict is deterministic.
    let (_root2, db2) = blog_fixture();
    let result = serve(ServerConfig::bind(addr.ip().to_string(), port), db2).await;

    let err = result
        .err()
        .expect("second bind on the same port must fail");
    assert!(
        matches!(&err, ServerError::Bind { addr, .. } if addr.contains(&port.to_string())),
        "expected a Bind error naming port {port}, got {err:?}",
    );

    first.shutdown().await.unwrap();
}
