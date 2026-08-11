//! Transport-agnostic `/query` pipeline.
//!
//! The full orchestration for running one query — intake validation,
//! the optional query timeout, [`DirSQL::query`], row serialization, and
//! error classification — lives here exactly once. The HTTP handler and
//! the one-shot `dirsql query` subcommand are thin transport adapters over
//! [`execute_query`], so the two surfaces cannot drift behaviorally:
//! per-surface code only maps [`QueryFailure`] to a status code or an
//! exit code.

use std::future::Future;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::AppState;
use super::serialize::rows_to_json;
use crate::{DirSQL, DirSqlError};

/// Why a query failed, classified independently of transport. The HTTP
/// adapter maps each arm to a status code (400 / 408 / 500 / 503); the CLI
/// adapter maps every arm to stderr + a non-zero exit.
#[derive(Debug)]
pub enum QueryFailure {
    /// Malformed input the caller can fix: unparsable body, missing/empty
    /// `sql`, or a SQL error from the core (HTTP 400).
    BadRequest(String),
    /// The query exceeded the configured timeout (HTTP 408).
    Timeout(Duration),
    /// A server-side fault: join error, lock poisoning (HTTP 500).
    Internal(String),
    /// The index never became ready — the degraded config state (HTTP 503).
    Unavailable(String),
}

impl QueryFailure {
    /// The diagnostic message, identical across transports (the HTTP
    /// `{"error": …}` body and the CLI stderr line).
    pub fn message(&self) -> String {
        match self {
            Self::Timeout(timeout) => format!("query exceeded {timeout:?} timeout"),
            Self::BadRequest(msg) | Self::Internal(msg) | Self::Unavailable(msg) => msg.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct QueryBody {
    sql: Option<String>,
}

/// Run one query end to end: parse the SQL from `raw_body` (`{"sql": …}`),
/// execute it against the index — bounded by `timeout` when one is given
/// (the server's 408 path), unbounded with `None` (the one-shot CLI, where
/// the process is the query and an external `timeout(1)` expresses any cap
/// natively) — and serialize the rows. `raw_body` is the exact payload a
/// `POST /query` request carries; the CLI adapter synthesizes the same
/// shape so both surfaces share intake validation.
pub async fn execute_query(
    state: &AppState,
    raw_body: String,
    timeout: Option<Duration>,
) -> Result<Value, QueryFailure> {
    let sql = parse_sql_body(&raw_body)?;

    let db = require_ready(state)?;

    let join = run_bounded(timeout, tokio::task::spawn_blocking(move || db.query(&sql))).await?;

    match join {
        Ok(Ok(rows)) => Ok(Value::Array(rows_to_json(&rows))),
        Ok(Err(err)) => Err(classify_query_error(err)),
        Err(join_err) => Err(QueryFailure::Internal(join_err.to_string())),
    }
}

/// Await `task`, bounded by `timeout` when one is given ([`QueryFailure::Timeout`]
/// carrying the bound on expiry) and to completion with `None`.
async fn run_bounded<T>(
    timeout: Option<Duration>,
    task: impl Future<Output = T>,
) -> Result<T, QueryFailure> {
    match timeout {
        Some(bound) => tokio::time::timeout(bound, task)
            .await
            .map_err(|_elapsed| QueryFailure::Timeout(bound)),
        None => Ok(task.await),
    }
}

/// Return a cloned [`DirSQL`] handle, or [`QueryFailure::Unavailable`] if
/// the index started in the degraded [`AppState::Unavailable`] state.
pub fn require_ready(state: &AppState) -> Result<DirSQL, QueryFailure> {
    match state {
        AppState::Ready(db) => Ok(db.clone()),
        AppState::Unavailable(reason) => Err(QueryFailure::Unavailable(reason.clone())),
    }
}

/// Parse a raw query body as `{"sql": …}` and return the trimmed SQL.
/// `BadRequest` on malformed JSON, and on a missing/empty `sql` field.
fn parse_sql_body(body: &str) -> Result<String, QueryFailure> {
    let parsed: QueryBody =
        serde_json::from_str(body).map_err(|err| QueryFailure::BadRequest(err.to_string()))?;
    match parsed.sql.as_deref().map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        Some(_) => Err(QueryFailure::BadRequest("`sql` must not be empty".into())),
        None => Err(QueryFailure::BadRequest("missing `sql` field".into())),
    }
}

/// SQL errors from the core are the caller's to fix (`BadRequest`); every
/// other failure (lock poisoning, watch/config faults) is server-side
/// (`Internal`).
fn classify_query_error(err: DirSqlError) -> QueryFailure {
    match err {
        DirSqlError::Core(_) | DirSqlError::WriteForbidden => {
            QueryFailure::BadRequest(err.to_string())
        }
        _ => QueryFailure::Internal(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `Core => BadRequest` arm is covered end-to-end by
    // `post_query_malformed_sql_returns_400_not_500` in
    // `tests/cli_integration.rs` (unit-lint isolation bars constructing a
    // `Core` value inline). The pure non-Core arm below stays here.

    #[test]
    fn classify_non_core_error_is_internal() {
        // Lock/watch/config failures are server-side faults -> `Internal`.
        // This drives the `_ =>` arm of `classify_query_error`.
        let err = DirSqlError::Lock("poisoned".into());
        let failure = classify_query_error(err);
        assert!(
            matches!(failure, QueryFailure::Internal(_)),
            "got: {failure:?}"
        );
    }

    #[test]
    fn classify_write_forbidden_is_bad_request() {
        // A rejected write is the caller's fault, exactly like a `Core` SQL
        // error -> `BadRequest`, not the `Internal` catch-all (issue #444).
        let failure = classify_query_error(DirSqlError::WriteForbidden);
        assert!(
            matches!(failure, QueryFailure::BadRequest(_)),
            "got: {failure:?}"
        );
    }

    #[test]
    fn require_ready_fails_unavailable_when_degraded() {
        // The degraded state yields `Unavailable` carrying the diagnostic
        // verbatim instead of a `DirSQL` handle.
        let state = AppState::Unavailable("config failed to load".into());
        // `DirSQL` isn't `Debug`, so go through `.err()` (which drops the Ok
        // value) rather than `expect_err`.
        let failure = require_ready(&state)
            .err()
            .expect("Unavailable must not yield a db");
        match failure {
            QueryFailure::Unavailable(reason) => assert_eq!(reason, "config failed to load"),
            other => panic!("expected Unavailable, got: {other:?}"),
        }
    }

    // `parse_sql_body` is the intake path: it is pure (serde only), so it is
    // unit-tested here directly rather than through the async `execute_query`
    // pipeline (which needs a live index and is covered at the integration
    // tier).

    #[test]
    fn parse_sql_body_returns_trimmed_sql() {
        // Surrounding whitespace is stripped; the inner SQL is returned as-is.
        let sql = parse_sql_body(r#"{"sql": "  SELECT 1  "}"#).expect("valid body");
        assert_eq!(sql, "SELECT 1");
    }

    #[test]
    fn parse_sql_body_rejects_malformed_json() {
        // A body that isn't JSON fails at the serde step -> `BadRequest`.
        let failure = parse_sql_body("not json").expect_err("malformed JSON must be rejected");
        assert!(
            matches!(failure, QueryFailure::BadRequest(_)),
            "got: {failure:?}"
        );
    }

    #[test]
    fn parse_sql_body_rejects_whitespace_only_sql() {
        // A present-but-blank `sql` trims to empty -> `BadRequest` (the
        // `Some(_)` arm, and the `false` side of the `!s.is_empty()` guard).
        let failure = parse_sql_body(r#"{"sql": "   "}"#).expect_err("empty sql must be rejected");
        match failure {
            QueryFailure::BadRequest(msg) => assert_eq!(msg, "`sql` must not be empty"),
            other => panic!("expected BadRequest, got: {other:?}"),
        }
    }

    #[test]
    fn parse_sql_body_rejects_missing_sql_field() {
        // Valid JSON object with no `sql` key -> the `None` arm -> `BadRequest`.
        let failure = parse_sql_body("{}").expect_err("missing sql must be rejected");
        match failure {
            QueryFailure::BadRequest(msg) => assert_eq!(msg, "missing `sql` field"),
            other => panic!("expected BadRequest, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn run_bounded_without_a_timeout_runs_to_completion() {
        // `None` is the one-shot CLI's mode: the task is awaited unbounded.
        let result = run_bounded(None, async { 42 }).await;
        assert!(matches!(result, Ok(42)), "got: {result:?}");
    }

    #[tokio::test]
    async fn run_bounded_with_an_unexpired_bound_yields_the_output() {
        let result = run_bounded(Some(Duration::from_secs(5)), async { "rows" }).await;
        assert!(matches!(result, Ok("rows")), "got: {result:?}");
    }

    #[tokio::test]
    async fn run_bounded_with_an_expired_bound_yields_timeout_carrying_it() {
        // The server's 408 path: an expired bound surfaces as `Timeout`
        // holding the exact duration, never the task's output.
        let bound = Duration::from_millis(1);
        let result = run_bounded(Some(bound), std::future::pending::<()>()).await;
        match result {
            Err(QueryFailure::Timeout(d)) => assert_eq!(d, bound),
            other => panic!("expected Timeout, got: {other:?}"),
        }
    }

    #[test]
    fn timeout_failure_message_names_the_duration() {
        // The timeout arm formats its message from the stored duration —
        // the exact text the HTTP 408 body carried before the extraction.
        let failure = QueryFailure::Timeout(Duration::from_secs(30));
        assert_eq!(failure.message(), "query exceeded 30s timeout");
    }

    #[test]
    fn non_timeout_failure_messages_pass_through_verbatim() {
        // The other three arms carry their diagnostic string as-is.
        assert_eq!(QueryFailure::BadRequest("a".into()).message(), "a");
        assert_eq!(QueryFailure::Internal("b".into()).message(), "b");
        assert_eq!(QueryFailure::Unavailable("c".into()).message(), "c");
    }
}
