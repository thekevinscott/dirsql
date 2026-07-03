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
    if let AppState::Ready(ref db) = state {
        start_watch_task(db.clone(), event_tx.clone());
    }

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let shared = Arc::new(AppContext {
        state,
        events: event_tx,
        cancel: cancel_rx,
        query_timeout: config.query_timeout,
        pre_query: config.pre_query,
        post_query: config.post_query,
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

fn start_watch_task(db: DirSQL, tx: broadcast::Sender<String>) {
    // `db.watch()` spawns its own OS thread and returns an async stream.
    // We pump the stream into the broadcast channel. If no subscribers
    // exist, send() errors but we keep pumping (future subscribers
    // will get subsequent events).
    let Ok(mut stream) = db.watch().map_err(|err| {
        eprintln!(
            "dirsql: failed to attach filesystem watcher ({err}); \
             /events will return an empty stream"
        );
    }) else {
        return;
    };
    tokio::spawn(async move {
        while let Some(event) = stream.next().await {
            let payload = event_to_json(&event);
            let _ = tx.send(payload);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    // Binding with an `Unavailable` state needs no DB/filesystem, so this
    // exercises the real bind / local-addr / graceful-shutdown plumbing
    // without standing up a full index. Port 0 lets the OS pick a free port.
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

    // A `Ready` state drives the `serve` convenience wrapper plus
    // `start_watch_task` (which attaches `DirSQL::watch` and pumps its stream
    // into the broadcast channel). Built over an empty temp dir so the scan
    // touches nothing; the port-0 bind and graceful shutdown are real.
    #[tokio::test]
    async fn serve_with_a_ready_db_attaches_the_watcher_then_shuts_down() {
        let dir = tempfile::tempdir().unwrap();
        let db = DirSQL::new(dir.path(), Vec::new()).unwrap();
        let config = ServerConfig::bind("127.0.0.1".to_string(), 0);
        let handle = serve(config, db).await.expect("bind on an ephemeral port");
        assert_ne!(handle.local_addr().port(), 0);
        handle.shutdown().await.expect("graceful shutdown");
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
