//! HTTP server backing the `dirsql` CLI.
//!
//! The surface is intentionally small:
//!
//! - [`serve`] — bind and start the server; returns a [`ServerHandle`] with
//!   `local_addr()` + `shutdown()`.
//! - [`ServerConfig`] — host / port / per-query timeout. Construct via
//!   `default()`, `ephemeral()`, or `bind(host, port)`.
//! - [`AppState`] — either a ready [`DirSQL`] or a degraded mode that
//!   returns 503 for every request. The binary uses the degraded mode
//!   when it fails to load `.dirsql.toml` so users can still connect to
//!   the HTTP server and see a diagnostic.
//!
//! Only available with `--features cli`. Each concern lives in its own
//! submodule:
//!
//! - [`server`] — bind/serve/shutdown plumbing.
//! - [`router`] — axum routes + request handlers.
//! - [`serialize`] — row + event → JSON.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::DirSQL;

pub mod init;
pub mod router;
pub mod serialize;
pub mod server;

pub use server::{serve, serve_with_state};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A server-wide `pre-query` command hook, carrying the command template plus
/// the directory it runs in (the config file's parent). When set on a
/// [`ServerConfig`], the server passes each `POST /query` request body to the
/// command as `{args}` and runs the plain-text SQL it prints. See
/// [`crate::command`] for the execution contract.
#[derive(Debug, Clone)]
pub struct PreQuery {
    /// The command template (argv-split, no shell). Receives the raw request
    /// body as the `{args}` placeholder.
    pub command: String,
    /// The command's working directory — the config file's parent.
    pub config_dir: PathBuf,
}

impl PreQuery {
    /// Build a [`PreQuery`] from a command template and its working directory.
    pub fn new(command: impl Into<String>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            config_dir: config_dir.into(),
        }
    }
}

/// A server-wide `post-query` command hook, carrying the command template plus
/// the directory it runs in (the config file's parent). When set on a
/// [`ServerConfig`], the server hands each successful `POST /query` result set
/// (the rows serialized as a JSON array) to the command as `{args}` and on
/// stdin, and returns the JSON body the command prints instead of the rows
/// as-is. See [`crate::command`] for the execution contract.
#[derive(Debug, Clone)]
pub struct PostQuery {
    /// The command template (argv-split, no shell). Receives the serialized
    /// result rows as the `{args}` placeholder (and on stdin).
    pub command: String,
    /// The command's working directory — the config file's parent.
    pub config_dir: PathBuf,
}

impl PostQuery {
    /// Build a [`PostQuery`] from a command template and its working directory.
    pub fn new(command: impl Into<String>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            config_dir: config_dir.into(),
        }
    }
}

/// Configure how the server binds. Defaults to `localhost:7117` with a
/// 30-second per-query timeout and no `pre-query` hook.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub query_timeout: Duration,
    /// Optional server-wide `pre-query` command. When `None` (the default),
    /// `POST /query` parses its body as `{"sql": …}`.
    pub pre_query: Option<PreQuery>,
    /// Optional server-wide `post-query` command. When `None` (the default),
    /// `POST /query` returns the result rows as-is.
    pub post_query: Option<PostQuery>,
}

impl ServerConfig {
    /// Bind an ephemeral TCP port on `localhost`. Convenient for tests;
    /// the real port is reachable via [`ServerHandle::local_addr`].
    pub fn ephemeral() -> Self {
        Self {
            host: "localhost".into(),
            port: 0,
            query_timeout: Duration::from_secs(30),
            pre_query: None,
            post_query: None,
        }
    }

    /// Bind `host:port` explicitly.
    pub fn bind(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            query_timeout: Duration::from_secs(30),
            pre_query: None,
            post_query: None,
        }
    }

    /// Override the per-query timeout. Requests exceeding this limit
    /// return `408 Request Timeout` and release the blocking thread.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// Attach a server-wide [`PreQuery`] hook. With it set, `POST /query`
    /// passes the raw request body to the command and runs the SQL it prints
    /// instead of parsing the body as `{"sql": …}`.
    pub fn with_pre_query(mut self, pre_query: PreQuery) -> Self {
        self.pre_query = Some(pre_query);
        self
    }

    /// Attach a server-wide [`PostQuery`] hook. With it set, `POST /query`
    /// hands each successful result set to the command and returns the JSON
    /// body it prints instead of returning the rows as-is.
    pub fn with_post_query(mut self, post_query: PostQuery) -> Self {
        self.post_query = Some(post_query);
        self
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self::bind("localhost", 7117)
    }
}

/// Degraded server state: if the binary couldn't load `.dirsql.toml`, it
/// starts the server in [`AppState::Unavailable`] so the HTTP endpoints
/// can report a clear 503 rather than failing to start entirely.
#[derive(Clone)]
pub enum AppState {
    Ready(DirSQL),
    Unavailable(String),
}

impl From<DirSQL> for AppState {
    fn from(db: DirSQL) -> Self {
        Self::Ready(db)
    }
}

/// Symmetric construction for the degraded arm. Lets call sites build the
/// degraded state with the same `.into()` ergonomics as the ready arm
/// instead of typing the variant name. The string is the diagnostic that
/// `/query` and `/events` echo back as a 503 body.
impl From<String> for AppState {
    fn from(reason: String) -> Self {
        Self::Unavailable(reason)
    }
}

/// Running server handle.
///
/// Always call [`shutdown`](Self::shutdown) to release the bound port
/// and drain in-flight requests; dropping the handle without shutdown
/// leaks a still-accepting `tokio::spawn`ed task.
#[must_use = "dropping the handle leaks the server task; call `.shutdown().await` to drain in-flight requests"]
pub struct ServerHandle {
    addr: SocketAddr,
    shutdown_tx: Option<oneshot::Sender<()>>,
    cancel_tx: watch::Sender<bool>,
    task: JoinHandle<Result<(), ServerError>>,
}

impl ServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// Trigger a graceful shutdown. Existing requests drain, SSE streams
    /// are cancelled, new connections are refused. Returns once the
    /// background task has exited.
    pub async fn shutdown(mut self) -> Result<(), ServerError> {
        // Signal SSE streams to close, then signal axum to stop accepting
        // new connections. With both signals delivered, any in-flight
        // queries complete and the server task exits.
        let _ = self.cancel_tx.send(true);
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        match self.task.await {
            Ok(result) => result,
            Err(err) => Err(ServerError::Join(err)),
        }
    }
}

/// Errors produced while binding or serving.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("bind {addr}: {source}")]
    Bind {
        addr: String,
        source: std::io::Error,
    },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("server task panicked: {0}")]
    Join(#[from] JoinError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // `From<DirSQL> for AppState` produces the `Ready` arm -- this is
    // verified at the integration tier by `from_dirsql_yields_ready_state`
    // in `tests/cli_integration.rs`, which builds a real `DirSQL` over a
    // temp directory (so the initial scan runs) and asserts the public
    // `AppState::Ready` variant. It lived here once but needed
    // `std::fs::write` to populate the scanned directory, which the
    // `testing-conventions` `unit lint` isolation rule forbids in a unit
    // test (effectful std). The pure config-default test below stays inline.

    #[test]
    fn default_config_binds_localhost_7117_with_30s_timeout() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 7117);
        assert_eq!(cfg.query_timeout, Duration::from_secs(30));
        assert!(cfg.pre_query.is_none());
        assert!(cfg.post_query.is_none());
    }

    #[test]
    fn pre_query_constructor_carries_command_and_dir() {
        // `PreQuery::new` is pure data plumbing: the command template and the
        // working directory it will run in.
        let pq = PreQuery::new("to_sql.py {args}", "/proj");
        assert_eq!(pq.command, "to_sql.py {args}");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
    }

    #[test]
    fn with_pre_query_sets_the_hook() {
        let cfg = ServerConfig::ephemeral().with_pre_query(PreQuery::new("cmd {args}", "/proj"));
        let pq = cfg.pre_query.expect("hook must be set");
        assert_eq!(pq.command, "cmd {args}");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
    }

    #[test]
    fn post_query_constructor_carries_command_and_dir() {
        // `PostQuery::new` is pure data plumbing: the command template and the
        // working directory it will run in.
        let pq = PostQuery::new("jq '{results: .}'", "/proj");
        assert_eq!(pq.command, "jq '{results: .}'");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
    }

    #[test]
    fn with_post_query_sets_the_hook() {
        let cfg =
            ServerConfig::ephemeral().with_post_query(PostQuery::new("reshape {args}", "/proj"));
        let pq = cfg.post_query.expect("hook must be set");
        assert_eq!(pq.command, "reshape {args}");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
    }
}
