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

use super::serialize::rows_to_json;
use super::{AppState, PostQuery, PreQuery};
use crate::command::{Placeholder, run_command};
use crate::{DirSQL, DirSqlError};

/// Cap on the serialized result payload passed as the `{args}` argv token.
/// Beyond this, `{args}` is emptied and the operator is directed to stdin
/// (which always carries the full payload) — comfortably under Linux's 128 KiB
/// single-arg `MAX_ARG_STRLEN`.
const POST_QUERY_ARGS_MAX: usize = 96 * 1024;

pub(super) struct AppContext {
    pub state: AppState,
    pub events: broadcast::Sender<String>,
    pub cancel: watch::Receiver<bool>,
    pub query_timeout: Duration,
    /// Optional server-wide `pre-query` hook. When `Some`, `POST /query`
    /// rewrites the request body through the command; when `None`, the body
    /// is parsed as `{"sql": …}`.
    pub pre_query: Option<PreQuery>,
    /// Optional server-wide `post-query` hook. When `Some`, a successful
    /// `POST /query` result set is reshaped by the command before responding;
    /// when `None`, the rows are returned as-is.
    pub post_query: Option<PostQuery>,
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

async fn handle_query(State(ctx): State<SharedCtx>, body: String) -> Response {
    // Resolve the SQL to run. With a `pre-query` hook the raw body is rewritten
    // by the command; without one it is parsed as `{"sql": …}` (today's path).
    let sql = match &ctx.pre_query {
        Some(pq) => match run_pre_query(pq, body).await {
            Ok(sql) => sql,
            Err(resp) => return resp,
        },
        None => match parse_sql_body(&body) {
            Ok(sql) => sql,
            Err(resp) => return resp,
        },
    };

    let db = match require_ready(&ctx.state) {
        Ok(db) => db,
        Err(resp) => return resp,
    };

    let timeout = ctx.query_timeout;
    let join =
        tokio::time::timeout(timeout, tokio::task::spawn_blocking(move || db.query(&sql))).await;

    match join {
        Ok(Ok(Ok(rows))) => {
            let rows_json = rows_to_json(&rows);
            match &ctx.post_query {
                Some(pq) => match run_post_query(pq, rows_json).await {
                    Ok(value) => Json(value).into_response(),
                    Err(resp) => resp,
                },
                None => Json(rows_json).into_response(),
            }
        }
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

/// Parse a `POST /query` body as `{"sql": …}` and return the trimmed SQL.
/// Reproduces the pre-hook behavior: 400 on malformed JSON, 400 on a
/// missing/empty `sql` field.
///
/// `Response` is large (clippy flags the error variant), but returning it
/// directly matches the axum handler contract and avoids boxing on the hot
/// path — same trade-off as [`require_ready`].
#[allow(clippy::result_large_err)]
fn parse_sql_body(body: &str) -> Result<String, Response> {
    let parsed: QueryBody = serde_json::from_str(body)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    match parsed.sql.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        Some(_) => Err(error_response(
            StatusCode::BAD_REQUEST,
            "`sql` must not be empty",
        )),
        None => Err(error_response(
            StatusCode::BAD_REQUEST,
            "missing `sql` field",
        )),
    }
}

/// Run the server-wide `pre-query` hook over the raw request body and return
/// the SQL it prints. The body is passed as the injection-safe `{args}`
/// placeholder (a single argv token); the command's last non-empty stdout line
/// is the SQL to run. Any failure (non-zero exit, timeout, spawn error) maps to
/// `500` carrying the command's stderr tail.
///
/// `Response` is large (see [`parse_sql_body`]); returned by value for the same
/// reason.
#[allow(clippy::result_large_err)]
async fn run_pre_query(pq: &PreQuery, raw_body: String) -> Result<String, Response> {
    let command = pq.command.clone();
    let config_dir = pq.config_dir.clone();
    let timeout = pq.timeout;
    // `run_command` is blocking — it spawns a child and joins drain threads —
    // so run it off the async runtime. It enforces the hook's timeout
    // (the global `[dirsql].hook-timeout`, default 30s) internally, so no outer
    // `tokio::time::timeout` is needed.
    let outcome = tokio::task::spawn_blocking(move || {
        run_command(
            &command,
            &[Placeholder::new("args", &raw_body)],
            &config_dir,
            timeout,
            None,
        )
    })
    .await
    .map_err(|join_err| error_response(StatusCode::INTERNAL_SERVER_ERROR, join_err.to_string()))?;

    // `run_command` only returns `Ok` with a non-empty last stdout line
    // (`EmptyOutput` otherwise), so the payload is the SQL as-is.
    outcome
        .map(|out| out.payload)
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

/// Run the server-wide `post-query` hook over a successful result set and
/// return the JSON body it prints. The rows are serialized to a JSON array and
/// delivered two ways: always on the child's stdin (unbounded, injection-safe),
/// and as the `{args}` placeholder when the payload is within
/// [`POST_QUERY_ARGS_MAX`] (beyond that `{args}` is emptied and a warning names
/// the size, directing the operator to stdin — never silent truncation). The
/// command's last non-empty stdout line is parsed as JSON and returned as the
/// `200` body; anything that isn't valid JSON, or any failure (non-zero exit,
/// timeout, spawn error), maps to `500`.
///
/// `Response` is large (see [`parse_sql_body`]); returned by value for the same
/// reason.
#[allow(clippy::result_large_err)]
async fn run_post_query(
    pq: &PostQuery,
    rows: Vec<serde_json::Value>,
) -> Result<serde_json::Value, Response> {
    let payload = serde_json::to_string(&rows)
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;
    let command = pq.command.clone();
    let config_dir = pq.config_dir.clone();
    let timeout = pq.timeout;
    // `run_command` is blocking — it spawns a child and joins drain threads —
    // so run it off the async runtime. It enforces the hook's timeout
    // (the global `[dirsql].hook-timeout`, default 30s) internally, so no outer
    // `tokio::time::timeout` is needed.
    let outcome = tokio::task::spawn_blocking(move || {
        let args_value = if payload.len() <= POST_QUERY_ARGS_MAX {
            payload.clone()
        } else {
            eprintln!(
                "dirsql: post-query result payload is {} bytes, exceeding the \
                 {POST_QUERY_ARGS_MAX}-byte argv threshold; `{{args}}` is emptied — \
                 read the rows from stdin instead",
                payload.len()
            );
            String::new()
        };
        run_command(
            &command,
            &[Placeholder::new("args", &args_value)],
            &config_dir,
            timeout,
            Some(payload.as_bytes()),
        )
    })
    .await
    .map_err(|join_err| error_response(StatusCode::INTERNAL_SERVER_ERROR, join_err.to_string()))?;

    let out = outcome
        .map_err(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    // The command's payload (last non-empty stdout line) is the JSON response
    // body; reject anything that doesn't parse as JSON.
    serde_json::from_str(&out.payload).map_err(|err| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("post-query did not return valid JSON: {err}"),
        )
    })
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
        Ok::<SseEvent, std::convert::Infallible>(SseEvent::default().event("ready").data("{}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    // The `DirSqlError::Core => 400` arm of `classify_query_error` is
    // exercised end-to-end at the integration tier by
    // `post_query_malformed_sql_returns_400_not_500` in
    // `tests/cli_integration.rs`, which posts malformed SQL to `/query` and
    // asserts the 400. Constructing a `Core` value inline would require
    // importing the first-party `crate::db::DbError`, which the
    // `testing-conventions` `unit lint` isolation rule forbids (a unit test
    // may reach only `super::` and pure `std`). The non-Core arm below is
    // pure -- it builds a `super::DirSqlError::Lock` -- so it stays inline.

    #[test]
    fn classify_non_core_error_is_internal_server_error() {
        // Lock/watch/config failures are server-side faults -> 500. This drives
        // the `_ =>` arm of `classify_query_error`.
        let err = DirSqlError::Lock("poisoned".into());
        assert_eq!(
            classify_query_error(&err),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn require_ready_returns_503_response_when_unavailable() {
        // The degraded state yields an error `Response` (rendered as 503 by
        // the HTTP layer) instead of a `DirSQL` handle.
        let state = AppState::Unavailable("config failed to load".into());
        // `DirSQL` isn't `Debug`, so go through `.err()` (which drops the Ok
        // value) rather than `expect_err`.
        let resp = require_ready(&state)
            .err()
            .expect("Unavailable must not yield a db");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_response_sets_json_content_type() {
        let resp = error_response(StatusCode::BAD_REQUEST, "boom");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
