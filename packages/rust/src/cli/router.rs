//! Axum router, request handlers, and shared context.

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures::stream::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::{broadcast, watch};
use tokio_stream::wrappers::BroadcastStream;

use super::AppState;
use super::serialize::rows_to_json;
use crate::{DirSQL, DirSqlError};

pub(super) struct AppContext {
    pub state: AppState,
    pub events: broadcast::Sender<String>,
    pub cancel: watch::Receiver<bool>,
    pub query_timeout: Duration,
}

pub(super) type SharedCtx = Arc<AppContext>;

pub(super) fn router(ctx: SharedCtx) -> Router {
    Router::new()
        .route(
            "/query",
            post(handle_query).on(axum::routing::MethodFilter::GET, method_not_allowed),
        )
        .route(
            "/events",
            get(handle_events).on(axum::routing::MethodFilter::POST, method_not_allowed),
        )
        .with_state(ctx)
}

#[derive(Debug, Deserialize)]
struct QueryBody {
    sql: Option<String>,
}

async fn handle_query(
    State(ctx): State<SharedCtx>,
    body: Result<Json<QueryBody>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let Json(body) = match body {
        Ok(body) => body,
        Err(rej) => return error_response(StatusCode::BAD_REQUEST, rej.body_text()),
    };

    let sql = match body.sql.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        Some(_) => return error_response(StatusCode::BAD_REQUEST, "`sql` must not be empty"),
        None => return error_response(StatusCode::BAD_REQUEST, "missing `sql` field"),
    };

    let db = match require_ready(&ctx.state) {
        Ok(db) => db,
        Err(resp) => return resp,
    };

    let timeout = ctx.query_timeout;
    let join = tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || db.query(&sql)),
    )
    .await;

    match join {
        Ok(Ok(Ok(rows))) => Json(rows_to_json(&rows)).into_response(),
        Ok(Ok(Err(err))) => {
            let status = classify_query_error(&err);
            error_response(status, err.to_string())
        }
        Ok(Err(join_err)) => {
            error_response(StatusCode::INTERNAL_SERVER_ERROR, join_err.to_string())
        }
        Err(_elapsed) => error_response(
            StatusCode::REQUEST_TIMEOUT,
            format!("query exceeded {:?} timeout", timeout),
        ),
    }
}

async fn handle_events(State(ctx): State<SharedCtx>) -> Response {
    if let Err(resp) = require_ready(&ctx.state) {
        return resp;
    }

    // Subscribe BEFORE anything that might block so we don't drop events
    // that fire between subscribing and the first poll.
    let rx = ctx.events.subscribe();
    let events = BroadcastStream::new(rx).filter_map(|res| async move {
        match res {
            Ok(data) => Some(Ok::<SseEvent, std::convert::Infallible>(
                SseEvent::default().event("row").data(data),
            )),
            // Lagging subscriber: skip missed events rather than terminating.
            Err(_) => None,
        }
    });

    // Yield a ready event up front so clients have a reliable signal that
    // the subscription is attached. Data is non-empty because SSE parsers
    // skip events with no `data:` line.
    let ready = futures::stream::once(async {
        Ok::<SseEvent, std::convert::Infallible>(
            SseEvent::default().event("ready").data("{}"),
        )
    });
    let combined = ready.chain(events);

    // Close the stream when the server's cancellation signal fires so
    // graceful shutdown actually completes (otherwise SSE streams hold
    // axum's in-flight count at > 0 indefinitely).
    let mut cancel = ctx.cancel.clone();
    let stream = combined.take_until(async move {
        let _ = cancel.wait_for(|v| *v).await;
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn method_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response()
}

/// Return a cloned [`DirSQL`] handle, or an error response if the
/// server started in [`AppState::Unavailable`].
///
/// `Response` is ~128 bytes — clippy flags the large-err variant, but
/// it matches axum's `IntoResponse` contract and avoids boxing on the
/// hot path.
#[allow(clippy::result_large_err)]
pub(super) fn require_ready(state: &AppState) -> Result<DirSQL, Response> {
    match state {
        AppState::Ready(db) => Ok(db.clone()),
        AppState::Unavailable(reason) => Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            reason.clone(),
        )),
    }
}

fn classify_query_error(err: &DirSqlError) -> StatusCode {
    match err {
        DirSqlError::Core(_) => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(super) fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    let body = json!({ "error": message.into() });
    let mut resp = (status, Json(body)).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp
}
