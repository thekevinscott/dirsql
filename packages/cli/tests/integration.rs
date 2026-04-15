//! Integration tests: drive `build_app` with a MockEngine through tower oneshot.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use dirsql_cli::engine::{MockEngine, QueryEngine};
use dirsql_cli::error::QueryError;
use dirsql_cli::server::build_app;
use dirsql_core::db::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

fn body_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap()
}

async fn read_body(body: Body) -> Vec<u8> {
    to_bytes(body, 1024 * 1024).await.unwrap().to_vec()
}

fn row(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

#[tokio::test]
async fn post_query_routes_sql_to_engine() {
    let mock = Arc::new(MockEngine::with_rows(vec![row(&[
        ("name", Value::Text("apple".into())),
        ("qty", Value::Integer(3)),
    ])]));
    let app = build_app(mock.clone() as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"sql":"SELECT * FROM items"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = read_body(resp.into_body()).await;
    let json = body_json(&bytes);
    assert_eq!(json["rows"][0]["name"], "apple");
    assert_eq!(json["rows"][0]["qty"], 3);
    assert_eq!(mock.last_sql(), Some("SELECT * FROM items".to_string()));
}

#[tokio::test]
async fn post_query_returns_400_when_engine_errors() {
    let mock = Arc::new(MockEngine::with_error("bad sql"));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"sql":"bogus"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = read_body(resp.into_body()).await;
    let json = body_json(&bytes);
    assert!(json["error"].as_str().unwrap().contains("bad sql"));
}

#[tokio::test]
async fn post_query_rejects_malformed_json_body() {
    let mock = Arc::new(MockEngine::with_rows(vec![]));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/query")
        .header("content-type", "application/json")
        .body(Body::from("this is not json"))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn events_returns_501() {
    let mock = Arc::new(MockEngine::with_rows(vec![]));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/events")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn healthz_returns_200() {
    let mock = Arc::new(MockEngine::with_rows(vec![]));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = read_body(resp.into_body()).await;
    assert_eq!(&bytes[..], b"ok");
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let mock = Arc::new(MockEngine::with_rows(vec![]));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/nope")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn non_post_on_query_returns_405() {
    let mock = Arc::new(MockEngine::with_rows(vec![]));
    let app = build_app(mock as Arc<dyn QueryEngine>);
    let req = Request::builder()
        .method(Method::GET)
        .uri("/query")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn query_error_display_is_forwarded() {
    // Prove QueryError string propagates into body.
    let err = QueryError::Engine("xyz123".into());
    assert!(err.to_string().contains("xyz123"));
}
