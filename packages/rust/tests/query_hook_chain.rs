//! Integration red tests for #546: `pre-query` / `post-query` hooks chain
//! FIFO across config entries.
//!
//! With N configs the hooks form pipelines in config order instead of
//! conflicting: request body → pre₁ → pre₂ → SQL, and rows → post₁ → post₂ →
//! response. Each stage receives the previous stage's output as `{args}`
//! (post-query stages also on stdin). A failing stage fails the request.
//!
//! Driven through `ServerConfig`'s hook builders: registering a second stage
//! must append, not replace. Today `with_pre_query` / `with_post_query`
//! overwrite the single `Option<_>` slot, so only the last stage runs and
//! every chain expectation fails on its assertions.
//!
//! Same harness as `cli_integration.rs`: in-process server, real HTTP.
#![cfg(feature = "cli")]

use dirsql::DirSQL;
use dirsql::cli::{PostQuery, PreQuery, ServerConfig, ServerHandle, serve};
use reqwest::StatusCode;
use serde_json::{Value as JsonValue, json};
use std::fs;
use tempfile::TempDir;

/// Build a `DirSQL` over a one-post blog fixture driven by `.dirsql.toml`.
fn blog_fixture() -> (TempDir, DirSQL) {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts/alice")).unwrap();
    fs::write(root.path().join("posts/alice/Hello-World.json"), "{}").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/{author}/{title}.json"
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

fn base_url(handle: &ServerHandle) -> String {
    format!("http://{}", handle.local_addr())
}

#[cfg(unix)]
fn write_script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    path
}

#[cfg(unix)]
#[tokio::test]
async fn two_pre_query_stages_chain_fifo() {
    let (_root, db) = blog_fixture();
    let scripts = TempDir::new().unwrap();
    // Stage 1 tags the payload; stage 2 turns the tagged payload into SQL.
    // The echoed value proves both stages ran, in declaration order.
    let stage1 = write_script(scripts.path(), "s1.sh", "echo \"$1+s1\"\n");
    let stage2 = write_script(
        scripts.path(),
        "s2.sh",
        "echo \"SELECT '$1+s2' AS echoed\"\n",
    );

    let config = ServerConfig::ephemeral()
        .with_pre_query(PreQuery::new(
            format!("sh {} {{args}}", stage1.display()),
            scripts.path(),
        ))
        .with_pre_query(PreQuery::new(
            format!("sh {} {{args}}", stage2.display()),
            scripts.path(),
        ));
    let handle = serve(config, db).await.expect("server should bind");

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .body("helloworld")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<JsonValue> = resp.json().await.unwrap();
    assert_eq!(
        body,
        vec![json!({"echoed": "helloworld+s1+s2"})],
        "both pre-query stages must run, FIFO: body -> stage1 -> stage2 -> SQL"
    );
    handle.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn two_post_query_stages_chain_fifo() {
    let (_root, db) = blog_fixture();
    let scripts = TempDir::new().unwrap();
    // Nested envelopes prove order: rows -> {"first": rows} -> {"second": {"first": rows}}.
    let stage1 = write_script(
        scripts.path(),
        "p1.sh",
        "data=$(cat)\necho \"{\\\"first\\\": $data}\"\n",
    );
    let stage2 = write_script(
        scripts.path(),
        "p2.sh",
        "data=$(cat)\necho \"{\\\"second\\\": $data}\"\n",
    );

    let config = ServerConfig::ephemeral()
        .with_post_query(PostQuery::new(
            format!("sh {} {{args}}", stage1.display()),
            scripts.path(),
        ))
        .with_post_query(PostQuery::new(
            format!("sh {} {{args}}", stage2.display()),
            scripts.path(),
        ));
    let handle = serve(config, db).await.expect("server should bind");

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .json(&json!({"sql": "SELECT title FROM posts"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: JsonValue = resp.json().await.unwrap();
    assert_eq!(
        body,
        json!({"second": {"first": [{"title": "Hello-World"}]}}),
        "both post-query stages must run, FIFO: rows -> stage1 -> stage2 -> response"
    );
    handle.shutdown().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_failing_earlier_stage_fails_the_request() {
    let (_root, db) = blog_fixture();
    let scripts = TempDir::new().unwrap();
    let stage1 = write_script(
        scripts.path(),
        "boom.sh",
        "echo boom-from-stage-one >&2\nexit 1\n",
    );
    let stage2 = write_script(scripts.path(), "s2.sh", "echo \"SELECT '$1' AS echoed\"\n");

    let config = ServerConfig::ephemeral()
        .with_pre_query(PreQuery::new(
            format!("sh {} {{args}}", stage1.display()),
            scripts.path(),
        ))
        .with_pre_query(PreQuery::new(
            format!("sh {} {{args}}", stage2.display()),
            scripts.path(),
        ));
    let handle = serve(config, db).await.expect("server should bind");

    let resp = reqwest::Client::new()
        .post(format!("{}/query", base_url(&handle)))
        .body("helloworld")
        .send()
        .await
        .unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "a failing earlier stage must fail the request, not be skipped"
    );
    let body: JsonValue = resp.json().await.unwrap();
    let msg = body
        .get("error")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    assert!(
        msg.contains("boom-from-stage-one"),
        "the diagnostic must surface the failing stage's stderr, got {body}"
    );
    handle.shutdown().await.unwrap();
}
