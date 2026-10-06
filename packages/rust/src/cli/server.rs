//! Bind / serve / shutdown plumbing.

use std::sync::Arc;

use futures::stream::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot, watch};

use super::router::{AppContext, router};
use super::serialize::event_to_json;
use super::{AppState, ServerConfig, ServerError, ServerHandle};
use crate::DirSQL;

/// Start the server with a ready [`DirSQL`]. Equivalent to
/// `serve_with_state(config, AppState::Ready(db))`.
pub async fn serve(config: ServerConfig, db: DirSQL) -> Result<ServerHandle, ServerError> {
    serve_with_state(config, AppState::Ready(db)).await
}

/// Start the server with an explicit [`AppState`]. The binary uses this
/// to bind even when `.dirsql.toml` failed to load — requests return 503
/// with the diagnostic captured in [`AppState::Unavailable`].
pub async fn serve_with_state(
    config: ServerConfig,
    state: AppState,
) -> Result<ServerHandle, ServerError> {
    let addr_str = format!("{}:{}", config.host, config.port);
    let listener = TcpListener::bind(&addr_str)
        .await
        .map_err(|source| ServerError::Bind {
            addr: addr_str.clone(),
            source,
        })?;
    let addr = listener.local_addr()?;

    // Start the watcher once, at bind time. Every /events subscriber fans
    // in via a broadcast channel — subsequent subscribers don't re-drain
    // the underlying notify watcher (which `DirSQL::watch` only permits
    // once per instance).
    let (event_tx, _) = broadcast::channel::<String>(256);
    let watch_failure = match state {
        AppState::Ready(ref db) => start_watch_task(db.clone(), event_tx.clone()),
        AppState::Unavailable(_) => None,
    };

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let shared = Arc::new(AppContext {
        state,
        events: event_tx,
        watch_failure,
        cancel: cancel_rx,
        query_timeout: config.query_timeout,
    });
    let app = router(shared);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(ServerError::from)
    });

    Ok(ServerHandle {
        addr,
        shutdown_tx: Some(shutdown_tx),
        cancel_tx,
        task,
    })
}

/// Returns the reason `/events` must refuse requests when the watcher
/// could not attach.
fn start_watch_task(db: DirSQL, tx: broadcast::Sender<String>) -> Option<String> {
    // `db.watch()` spawns its own OS thread and returns an async stream.
    // We pump the stream into the broadcast channel. If no subscribers
    // exist, send() errors but we keep pumping (future subscribers
    // will get subsequent events).
    let mut stream = match db.watch() {
        Ok(stream) => stream,
        Err(err) => {
            let reason = format!("filesystem watcher failed to start: {err}");
            eprintln!("dirsql: {reason}; /events will return 503");
            return Some(reason);
        }
    };
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let payload = event_to_json(&event);
            let _ = tx.send(payload);
        }
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // An `Unavailable` state needs no DB/filesystem, so the real bind /
    // graceful-shutdown plumbing runs without standing up an index.
    #[tokio::test]
    async fn serve_with_state_binds_an_ephemeral_port_then_shuts_down() {
        let config = ServerConfig::bind("127.0.0.1".to_string(), 0);
        let handle = serve_with_state(config, AppState::Unavailable("test".to_string()))
            .await
            .expect("bind on an ephemeral port");

        assert_ne!(
            handle.local_addr().port(),
            0,
            "the OS should have assigned a concrete port"
        );

        handle.shutdown().await.expect("graceful shutdown");
    }

    #[tokio::test]
    async fn serve_with_a_ready_db_attaches_the_watcher_then_shuts_down() {
        let dir = tempfile::tempdir().unwrap();
        let db = DirSQL::new(dir.path(), Vec::new()).unwrap();
        let config = ServerConfig::bind("127.0.0.1".to_string(), 0);
        let handle = serve(config, db).await.expect("bind on an ephemeral port");
        assert_ne!(handle.local_addr().port(), 0);
        handle.shutdown().await.expect("graceful shutdown");
    }

    #[tokio::test]
    async fn start_watch_task_reports_no_failure_once_the_watcher_attaches() {
        let dir = tempfile::tempdir().unwrap();
        let db = DirSQL::new(dir.path(), Vec::new()).unwrap();
        let (tx, _) = broadcast::channel::<String>(1);
        assert_eq!(start_watch_task(db, tx), None);
    }

    // `poll_events` locks out `watch`, the one deterministic way to make the
    // watcher refuse to attach without exhausting inotify.
    #[tokio::test]
    async fn start_watch_task_returns_the_reason_when_the_watcher_cannot_attach() {
        let dir = tempfile::tempdir().unwrap();
        let db = DirSQL::new(dir.path(), Vec::new()).unwrap();
        db.poll_events(std::time::Duration::ZERO).unwrap();
        let (tx, _) = broadcast::channel::<String>(1);
        let reason = start_watch_task(db, tx).expect("watch() must fail");
        assert!(
            reason.starts_with("filesystem watcher failed to start: "),
            "got: {reason}"
        );
    }

    // Binding to a non-local TEST-NET address (RFC 5737) fails with
    // "cannot assign requested address", surfacing `ServerError::Bind` rather
    // than panicking — deterministic and DNS-free.
    #[tokio::test]
    async fn serve_with_state_surfaces_a_bind_error_for_a_nonlocal_address() {
        let config = ServerConfig::bind("192.0.2.1".to_string(), 9);
        // `serve_with_state` returns `Result<ServerHandle, _>`; ServerHandle is
        // not Debug, so match rather than `unwrap_err`.
        let err = match serve_with_state(config, AppState::Unavailable("x".to_string())).await {
            Ok(_) => panic!("expected a bind error"),
            Err(e) => e,
        };
        assert!(matches!(err, ServerError::Bind { .. }), "got: {err:?}");
    }
}
