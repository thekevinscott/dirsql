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
//! - [`router`] — axum routes + request handlers (thin HTTP adapters).
//! - [`execute`] — the transport-agnostic query pipeline shared by the
//!   HTTP handler and the one-shot `dirsql query` subcommand.
//! - [`serialize`] — row + event → JSON.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::DirSQL;
use crate::command::DEFAULT_COMMAND_TIMEOUT;

pub mod execute;
pub mod init;
pub mod router;
pub mod serialize;
pub mod server;

pub use server::{serve, serve_with_state};

/// The one starter `.dirsql.toml` -- a single `files` table over every file
/// under the root, built from the seven stat columns. This is both what
/// `dirsql init` writes verbatim ([`init::run`]) and what zero-config mode
/// parses to build its default table, so the two can never drift apart.
pub const DEFAULT_CONFIG_TOML: &str = include_str!("../default_config.toml");

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
    /// Per-run timeout. Defaults to the shared 30-second
    /// [`DEFAULT_COMMAND_TIMEOUT`]; override it via [`Self::with_timeout`]
    /// (the CLI wires the global `[dirsql].hook-timeout` here).
    pub timeout: Duration,
}

impl PreQuery {
    /// Build a [`PreQuery`] from a command template and its working directory,
    /// with the default 30-second timeout.
    pub fn new(command: impl Into<String>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            config_dir: config_dir.into(),
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    /// Override the per-run timeout (from the global `[dirsql].hook-timeout`).
    /// A run exceeding it is killed and the request returns 500.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
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
    /// Per-run timeout. Defaults to the shared 30-second
    /// [`DEFAULT_COMMAND_TIMEOUT`]; override it via [`Self::with_timeout`]
    /// (the CLI wires the global `[dirsql].hook-timeout` here).
    pub timeout: Duration,
}

impl PostQuery {
    /// Build a [`PostQuery`] from a command template and its working directory,
    /// with the default 30-second timeout.
    pub fn new(command: impl Into<String>, config_dir: impl Into<PathBuf>) -> Self {
        Self {
            command: command.into(),
            config_dir: config_dir.into(),
            timeout: DEFAULT_COMMAND_TIMEOUT,
        }
    }

    /// Override the per-run timeout (from the global `[dirsql].hook-timeout`).
    /// A run exceeding it is killed and the request returns 500.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Configure how the server binds. Defaults to `localhost:7117` with a
/// 30-second per-query timeout and no `pre-query` hook.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub query_timeout: Duration,
    /// Ordered `pre-query` command chain. Empty (the default) means
    /// `POST /query` parses its body as `{"sql": …}`; otherwise the raw body is
    /// piped through each stage in registration order (body → stage₁ → … → SQL),
    /// each stage receiving the previous stage's output as its `{args}`.
    pub pre_query: Vec<PreQuery>,
    /// Ordered `post-query` command chain. Empty (the default) means
    /// `POST /query` returns the result rows as-is; otherwise the rows are piped
    /// through each stage in registration order (rows → stage₁ → … → response),
    /// each stage receiving the previous stage's output as its `{args}` (and on
    /// stdin).
    pub post_query: Vec<PostQuery>,
}

impl ServerConfig {
    /// Bind an ephemeral TCP port on `localhost`. Convenient for tests;
    /// the real port is reachable via [`ServerHandle::local_addr`].
    pub fn ephemeral() -> Self {
        Self {
            host: "localhost".into(),
            port: 0,
            query_timeout: Duration::from_secs(30),
            pre_query: Vec::new(),
            post_query: Vec::new(),
        }
    }

    /// Bind `host:port` explicitly.
    pub fn bind(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            query_timeout: Duration::from_secs(30),
            pre_query: Vec::new(),
            post_query: Vec::new(),
        }
    }

    /// Override the per-query timeout. Requests exceeding this limit
    /// return `408 Request Timeout` and release the blocking thread.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
        self
    }

    /// Append a [`PreQuery`] stage to the chain. Stages run in registration
    /// order (body → stage₁ → … → SQL); the first stage receives the raw
    /// request body and each subsequent stage receives the previous stage's
    /// output, the last stage's output being the SQL to run. With no stage the
    /// body is parsed as `{"sql": …}`.
    pub fn with_pre_query(mut self, pre_query: PreQuery) -> Self {
        self.pre_query.push(pre_query);
        self
    }

    /// Append a [`PostQuery`] stage to the chain. Stages run in registration
    /// order (rows → stage₁ → … → response); the first stage receives the
    /// serialized result rows and each subsequent stage receives the previous
    /// stage's output, the last stage's output being the response body. With no
    /// stage the rows are returned as-is.
    pub fn with_post_query(mut self, post_query: PostQuery) -> Self {
        self.post_query.push(post_query);
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

/// The string is the diagnostic that `/query` and `/events` echo back as a
/// 503 body.
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

    // `From<DirSQL> for AppState` needs a real scanned directory, so its
    // `Ready`-arm test lives in `tests/cli_integration.rs` (unit-lint
    // isolation forbids the fs setup here).

    #[test]
    fn default_config_binds_localhost_7117_with_30s_timeout() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 7117);
        assert_eq!(cfg.query_timeout, Duration::from_secs(30));
        assert!(cfg.pre_query.is_empty());
        assert!(cfg.post_query.is_empty());
    }

    #[test]
    fn pre_query_constructor_carries_command_and_dir() {
        let pq = PreQuery::new("to_sql.py {args}", "/proj");
        assert_eq!(pq.command, "to_sql.py {args}");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
        assert_eq!(pq.timeout, Duration::from_secs(30));
    }

    #[test]
    fn pre_query_with_timeout_overrides_the_default() {
        let pq = PreQuery::new("cmd {args}", "/proj").with_timeout(Duration::from_secs(60));
        assert_eq!(pq.timeout, Duration::from_secs(60));
    }

    #[test]
    fn with_pre_query_appends_stages_in_order() {
        let cfg = ServerConfig::ephemeral()
            .with_pre_query(PreQuery::new("first {args}", "/a"))
            .with_pre_query(PreQuery::new("second {args}", "/b"));
        assert_eq!(cfg.pre_query.len(), 2);
        assert_eq!(cfg.pre_query[0].command, "first {args}");
        assert_eq!(cfg.pre_query[0].config_dir, PathBuf::from("/a"));
        assert_eq!(cfg.pre_query[1].command, "second {args}");
        assert_eq!(cfg.pre_query[1].config_dir, PathBuf::from("/b"));
    }

    #[test]
    fn post_query_constructor_carries_command_and_dir() {
        let pq = PostQuery::new("jq '{results: .}'", "/proj");
        assert_eq!(pq.command, "jq '{results: .}'");
        assert_eq!(pq.config_dir, PathBuf::from("/proj"));
        assert_eq!(pq.timeout, Duration::from_secs(30));
    }

    #[test]
    fn post_query_with_timeout_overrides_the_default() {
        let pq = PostQuery::new("reshape {args}", "/proj").with_timeout(Duration::from_secs(60));
        assert_eq!(pq.timeout, Duration::from_secs(60));
    }

    #[test]
    fn with_post_query_appends_stages_in_order() {
        let cfg = ServerConfig::ephemeral()
            .with_post_query(PostQuery::new("first {args}", "/a"))
            .with_post_query(PostQuery::new("second {args}", "/b"));
        assert_eq!(cfg.post_query.len(), 2);
        assert_eq!(cfg.post_query[0].command, "first {args}");
        assert_eq!(cfg.post_query[0].config_dir, PathBuf::from("/a"));
        assert_eq!(cfg.post_query[1].command, "second {args}");
        assert_eq!(cfg.post_query[1].config_dir, PathBuf::from("/b"));
    }

    #[test]
    fn with_query_timeout_overrides_the_default() {
        let cfg = ServerConfig::bind("127.0.0.1", 8080).with_query_timeout(Duration::from_secs(5));
        assert_eq!(cfg.query_timeout, Duration::from_secs(5));
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 8080);
    }

    #[test]
    fn app_state_from_string_builds_the_unavailable_arm() {
        // `AppState` isn't `Debug`, so match instead of asserting on a rendering.
        let state: AppState = "config failed to load".to_string().into();
        match state {
            AppState::Unavailable(reason) => assert_eq!(reason, "config failed to load"),
            AppState::Ready(_) => panic!("String must map to the Unavailable arm"),
        }
    }

    // `DEFAULT_CONFIG_TOML` parsing is covered in `tests/config.rs`
    // (unit-lint isolation bars calling `config::load_config_str` here).
}
