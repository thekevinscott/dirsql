//! Axum router, request handlers, and shared context.
//!
//! Handlers are thin transport adapters: the whole query pipeline lives in
//! [`super::execute`], and this module only maps its outcome onto HTTP —
//! [`QueryFailure`] arms onto status codes, the success value onto a JSON
//! body.

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
use serde_json::json;
use tokio::sync::{broadcast, watch};
use tokio_stream::wrappers::BroadcastStream;

use super::execute::{QueryFailure, execute_query, require_ready};
use super::{AppState, PostQuery, PreQuery};

pub(super) struct AppContext {
    pub state: AppState,
    pub events: broadcast::Sender<String>,
    pub cancel: watch::Receiver<bool>,
    pub query_timeout: Duration,
    /// Ordered `pre-query` command chain. Empty means `POST /query` parses the
    /// body as `{"sql": …}`; otherwise the raw body is piped through each stage
    /// in order (body → stage₁ → … → SQL).
    pub pre_query: Vec<PreQuery>,
    /// Ordered `post-query` command chain. Empty means the rows are returned
    /// as-is; otherwise a successful result set is piped through each stage in
    /// order (rows → stage₁ → … → response).
    pub post_query: Vec<PostQuery>,
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

async fn handle_query(State(ctx): State<SharedCtx>, body: String) -> Response {
    match execute_query(
        &ctx.state,
        body,
        ctx.query_timeout,
        &ctx.pre_query,
        &ctx.post_query,
    )
    .await
    {
        Ok(value) => Json(value).into_response(),
        Err(failure) => failure_response(&failure),
    }
}

/// The HTTP status for each [`QueryFailure`] arm. This mapping — together
/// with [`failure_response`]'s `{"error": …}` body — is the entirety of the
/// HTTP adapter's own behavior; everything else is the shared pipeline.
fn failure_status(failure: &QueryFailure) -> StatusCode {
    match failure {
        QueryFailure::BadRequest(_) => StatusCode::BAD_REQUEST,
        QueryFailure::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
        QueryFailure::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        QueryFailure::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn failure_response(failure: &QueryFailure) -> Response {
    error_response(failure_status(failure), failure.message())
}

async fn handle_events(State(ctx): State<SharedCtx>) -> Response {
    if let Err(failure) = require_ready(&ctx.state) {
        return failure_response(&failure);
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

    // The pipeline itself (intake validation, hooks, timeout, execution,
    // error classification) is unit-tested in `cli/execute.rs` and covered
    // end-to-end by `tests/cli_integration.rs`. What remains here is the
    // HTTP adapter's own behavior: the failure -> status mapping and the
    // JSON error body.

    #[test]
    fn failure_status_maps_each_arm_to_its_http_status() {
        assert_eq!(
            failure_status(&QueryFailure::BadRequest("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            failure_status(&QueryFailure::Timeout(Duration::from_secs(30))),
            StatusCode::REQUEST_TIMEOUT
        );
        assert_eq!(
            failure_status(&QueryFailure::Internal("x".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            failure_status(&QueryFailure::Unavailable("x".into())),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn failure_response_carries_the_status_and_message() {
        // The degraded arm renders as the 503 JSON error the server always
        // returned; the message travels through `QueryFailure::message`.
        let resp = failure_response(&QueryFailure::Unavailable("config failed to load".into()));
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            resp.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
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
