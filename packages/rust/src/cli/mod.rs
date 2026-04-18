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
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::DirSQL;

pub mod router;
pub mod serialize;
pub mod server;

pub use server::{serve, serve_with_state};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configure how the server binds. Defaults to `localhost:7117` with a
/// 30-second per-query timeout.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub query_timeout: Duration,
}

impl ServerConfig {
    /// Bind an ephemeral TCP port on `localhost`. Convenient for tests;
    /// the real port is reachable via [`ServerHandle::local_addr`].
    pub fn ephemeral() -> Self {
        Self {
            host: "localhost".into(),
            port: 0,
            query_timeout: Duration::from_secs(30),
        }
    }

    /// Bind `host:port` explicitly.
    pub fn bind(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            query_timeout: Duration::from_secs(30),
        }
    }

    /// Override the per-query timeout. Requests exceeding this limit
    /// return `408 Request Timeout` and release the blocking thread.
    pub fn with_query_timeout(mut self, timeout: Duration) -> Self {
        self.query_timeout = timeout;
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
