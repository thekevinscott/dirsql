//! Transport-agnostic `/query` pipeline.
//!
//! The full orchestration for running one query — intake validation,
//! the `pre-query` hook, the query timeout, [`DirSQL::query`], row
//! serialization, the `post-query` hook, and error classification —
//! lives here exactly once. The HTTP handler and the one-shot
//! `dirsql query` subcommand are thin transport adapters over
//! [`execute_query`], so the two surfaces cannot drift behaviorally:
//! per-surface code only maps [`QueryFailure`] to a status code or an
//! exit code.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::serialize::rows_to_json;
use super::{AppState, PostQuery, PreQuery};
use crate::command::{Placeholder, run_command};
use crate::{DirSQL, DirSqlError};

/// Cap on the serialized result payload passed as the `{args}` argv token.
/// Beyond this, `{args}` is emptied and the operator is directed to stdin
/// (which always carries the full payload) — comfortably under Linux's 128 KiB
/// single-arg `MAX_ARG_STRLEN`.
const POST_QUERY_ARGS_MAX: usize = 96 * 1024;

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
    /// A server-side fault: hook failure, join error, lock poisoning
    /// (HTTP 500).
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

/// Run one query end to end: resolve the SQL from `raw_body` (through the
/// `pre-query` hook when present, else parsed as `{"sql": …}`), execute it
/// against the index under `timeout`, serialize the rows, and reshape them
/// through the `post-query` hook when present. `raw_body` is the exact
/// payload a `POST /query` request carries; the CLI adapter synthesizes the
/// same shape so both surfaces share intake validation and hook semantics.
pub async fn execute_query(
    state: &AppState,
    raw_body: String,
    timeout: Duration,
    pre_query: Option<&PreQuery>,
    post_query: Option<&PostQuery>,
) -> Result<Value, QueryFailure> {
    // Resolve the SQL to run. With a `pre-query` hook the raw body is
    // rewritten by the command; without one it is parsed as `{"sql": …}`.
    let sql = match pre_query {
        Some(pq) => run_pre_query(pq, raw_body).await?,
        None => parse_sql_body(&raw_body)?,
    };

    let db = require_ready(state)?;

    let join =
        tokio::time::timeout(timeout, tokio::task::spawn_blocking(move || db.query(&sql))).await;

    match join {
        Ok(Ok(Ok(rows))) => {
            let rows_json = rows_to_json(&rows);
            match post_query {
                Some(pq) => run_post_query(pq, rows_json).await,
                None => Ok(Value::Array(rows_json)),
            }
        }
        Ok(Ok(Err(err))) => Err(classify_query_error(err)),
        Ok(Err(join_err)) => Err(QueryFailure::Internal(join_err.to_string())),
        Err(_elapsed) => Err(QueryFailure::Timeout(timeout)),
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

/// Run the `pre-query` hook over the raw request body and return the SQL it
/// prints. The body is passed as the injection-safe `{args}` placeholder (a
/// single argv token); the command's last non-empty stdout line is the SQL to
/// run. Any failure (non-zero exit, timeout, spawn error) maps to `Internal`
/// carrying the command's stderr tail.
async fn run_pre_query(pq: &PreQuery, raw_body: String) -> Result<String, QueryFailure> {
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
    .map_err(|join_err| QueryFailure::Internal(join_err.to_string()))?;

    // `run_command` only returns `Ok` with a non-empty last stdout line
    // (`EmptyOutput` otherwise), so the payload is the SQL as-is.
    outcome
        .map(|out| out.payload)
        .map_err(|err| QueryFailure::Internal(err.to_string()))
}

/// Run the `post-query` hook over a successful result set and return the JSON
/// body it prints. The rows are serialized to a JSON array and delivered two
/// ways: always on the child's stdin (unbounded, injection-safe), and as the
/// `{args}` placeholder when the payload is within [`POST_QUERY_ARGS_MAX`]
/// (beyond that `{args}` is emptied and a warning names the size, directing
/// the operator to stdin — never silent truncation). The command's last
/// non-empty stdout line is parsed as JSON and returned as the result;
/// anything that isn't valid JSON, or any failure (non-zero exit, timeout,
/// spawn error), maps to `Internal`.
async fn run_post_query(pq: &PostQuery, rows: Vec<Value>) -> Result<Value, QueryFailure> {
    let payload =
        serde_json::to_string(&rows).map_err(|err| QueryFailure::Internal(err.to_string()))?;
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
    .map_err(|join_err| QueryFailure::Internal(join_err.to_string()))?;

    let out = outcome.map_err(|err| QueryFailure::Internal(err.to_string()))?;

    // The command's payload (last non-empty stdout line) is the JSON result
    // body; reject anything that doesn't parse as JSON.
    serde_json::from_str(&out.payload).map_err(|err| {
        QueryFailure::Internal(format!("post-query did not return valid JSON: {err}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // The `DirSqlError::Core => BadRequest` arm of `classify_query_error` is
    // exercised end-to-end at the integration tier by
    // `post_query_malformed_sql_returns_400_not_500` in
    // `tests/cli_integration.rs`, which posts malformed SQL to `/query` and
    // asserts the 400. Constructing a `Core` value inline would require
    // importing the first-party `crate::db::DbError`, which the
    // `testing-conventions` `unit lint` isolation rule forbids (a unit test
    // may reach only `super::` and pure `std`). The non-Core arm below is
    // pure -- it builds a `super::DirSqlError::Lock` -- so it stays inline.

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

    // `parse_sql_body` is the no-`pre-query` intake path: it is pure (serde
    // only), so it is unit-tested here directly rather than through the async
    // `execute_query` pipeline (which needs a live index and is covered at
    // the integration tier).

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
