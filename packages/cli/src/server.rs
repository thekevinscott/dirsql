//! HTTP server built on axum.

use crate::engine::{QueryEngine, row_to_json};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct QueryBody {
    sql: String,
}

#[derive(Clone)]
struct AppState {
    engine: Arc<dyn QueryEngine>,
}

pub fn build_app(engine: Arc<dyn QueryEngine>) -> Router {
    let state = AppState { engine };
    Router::new()
        .route("/query", post(query_handler))
        .route("/healthz", get(healthz))
        .route("/events", get(events_stub))
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn events_stub() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        "events stream not implemented yet; see dirsql roadmap for SSE/WebSocket/JSONL options",
    )
}

async fn query_handler(
    State(state): State<AppState>,
    body: Result<Json<QueryBody>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let Json(body) = match body {
        Ok(b) => b,
        Err(rej) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": rej.body_text() })),
            )
                .into_response();
        }
    };

    let engine = state.engine.clone();
    let sql = body.sql;
    let rows_result = tokio::task::spawn_blocking(move || engine.query(&sql)).await;

    match rows_result {
        Ok(Ok(rows)) => {
            let arr: Vec<serde_json::Value> = rows.iter().map(row_to_json).collect();
            (StatusCode::OK, Json(serde_json::json!({ "rows": arr }))).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": format!("join error: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MockEngine;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn healthz_body_is_ok() {
        let app = build_app(Arc::new(MockEngine::with_rows(vec![])));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let b = to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&b[..], b"ok");
    }

    #[tokio::test]
    async fn query_returns_content_type_json() {
        let app = build_app(Arc::new(MockEngine::with_rows(vec![])));
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/query")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"sql":"SELECT 1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("application/json"));
    }
}
