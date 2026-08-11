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
use std::time::Duration;

use tokio::sync::{oneshot, watch};
use tokio::task::JoinError;
use tokio::task::JoinHandle;

use crate::DirSQL;

pub mod execute;
pub mod init;
pub mod router;
pub mod run;
pub mod serialize;
pub mod server;

pub use run::run_cli;
pub use server::{serve, serve_with_state};

/// Re-export of the crate-level baked-in default config
/// ([`crate::DEFAULT_CONFIG_TOML`]). This is both what `dirsql init` writes
/// verbatim ([`init::run`]) and what the CLI's no-`-c` default serves, so the
/// two can never drift apart.
pub use crate::DEFAULT_CONFIG_TOML;

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
