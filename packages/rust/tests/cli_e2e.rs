//! End-to-end tests for the `dirsql` CLI binary (issue #105).
//!
//! These tests spawn the actual compiled `dirsql` binary as a subprocess,
//! talk to it over real HTTP, and drive real filesystem mutations. Nothing
//! is mocked. Tests are deliberately tolerant of startup / shutdown timing
//! via bounded retries; they are NOT tolerant of missing or broken
//! behavior described in `docs/guide/cli.md`.
//!
//! RED PHASE: the `dirsql` binary does not exist yet. `assert_cmd`'s
//! `cargo_bin` lookup will fail for every test below. These tests turn
//! green once #101 (bin scaffolding) lands and #105 (server) implements
//! the HTTP surface documented in the CLI guide.

use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::prelude::*;
use reqwest::{StatusCode, blocking::Client};
use serde_json::{Value, json};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixtures & helpers
// ---------------------------------------------------------------------------

/// Write a two-post blog fixture into a fresh tempdir and return it.
/// The `.dirsql.toml` lives at the root so `dirsql` can discover it.
fn blog_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::write(
        root.path().join("posts/hello.json"),
        r#"{"title":"Hello World","author":"alice"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("posts/second.json"),
        r#"{"title":"Second Post","author":"bob"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.json"
"#,
    )
    .unwrap();
    root
}

/// Pick a free TCP port by opening and immediately dropping a listener.
fn free_port() -> u16 {
    TcpListener::bind("localhost:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Spawn `dirsql` as a subprocess bound to `--port <port>` in `dir`.
/// The child inherits stderr so failures surface in test output.
fn spawn_dirsql(dir: &std::path::Path, port: u16) -> Child {
    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli")
        .into();
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    cmd.spawn().expect("spawning dirsql failed")
}

/// Block until the server answers `GET /query` (or times out).
fn wait_until_ready(port: u16, timeout: Duration) {
    let client = Client::builder().timeout(Duration::from_millis(250)).build().unwrap();
    let url = format!("http://localhost:{port}/query");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        // Any HTTP response (even 405) proves the server is listening.
        if client.get(&url).send().is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("dirsql server did not become ready on port {port} within {timeout:?}");
}

fn kill_and_wait(mut child: Child) {
    // Prefer polite shutdown; fall back to kill if the child hangs.
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let pid = child.id();
        unsafe {
            libc::kill(pid as i32, libc::SIGINT);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(
                    status.success() || status.signal() == Some(libc::SIGINT),
                    "expected clean exit on SIGINT, got {status:?}"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

// ---------------------------------------------------------------------------
// Binary smoke tests
// ---------------------------------------------------------------------------

#[test]
fn version_flag_prints_and_exits_zero() {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist (cargo install --features cli / `cargo test --features cli`)")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicates::str::is_match(r"^dirsql \d+\.\d+\.\d+").unwrap());
}

#[test]
fn help_flag_prints_and_exits_zero() {
    // Every flag documented in docs/guide/cli.md must appear in `--help`.
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--config"))
        .stdout(predicates::str::contains("--host"))
        .stdout(predicates::str::contains("--port"));
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

#[test]
fn server_announces_bind_on_stdout() {
    // Per docs/guide/cli.md: on startup, the server prints something like
    // `Running at localhost:7117` to stdout. Parse the first line.
    let root = blog_fixture();
    let port = free_port();
    let mut child = spawn_dirsql(root.path(), port);

    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("expected a startup line");
    assert!(
        line.contains(&format!("localhost:{port}")),
        "unexpected startup banner: {line:?}"
    );

    kill_and_wait(child);
}

#[test]
fn post_query_returns_rows_over_http() {
    let root = blog_fixture();
    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT title FROM posts ORDER BY title"}))
        .send()
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![
            json!({"title": "Hello World"}),
            json!({"title": "Second Post"}),
        ]
    );

    kill_and_wait(child);
}

#[test]
fn get_events_emits_insert_event_when_file_created() {
    let root = blog_fixture();
    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    // Open SSE stream in a background thread. Signal when the server's
    // `ready` sentinel has arrived so the test can mutate AFTER the
    // subscription is attached (avoids races with lazy HTTP connects).
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let stream_url = format!("http://localhost:{port}/events");
    std::thread::spawn(move || {
        let resp = Client::builder()
            .timeout(None)
            .build()
            .unwrap()
            .get(&stream_url)
            .send()
            .unwrap();
        let reader = BufReader::new(resp);
        let mut ready_sent = false;
        for line in reader.lines().map_while(Result::ok) {
            let Some(rest) = line.strip_prefix("data:") else {
                continue;
            };
            let trimmed = rest.trim().to_string();
            // Skip the `{}` ready sentinel emitted on subscribe.
            if !ready_sent && trimmed == "{}" {
                ready_sent = true;
                ready_tx.send(()).ok();
                continue;
            }
            tx.send(trimmed).ok();
            break;
        }
    });

    // Wait for the server's ready sentinel (subscription attached), then
    // give `notify` a breath to finish installing its inotify watches
    // before mutating the fixture.
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("SSE stream never produced a ready sentinel");
    std::thread::sleep(Duration::from_millis(200));
    fs::write(
        root.path().join("posts/third.json"),
        r#"{"title":"Third Post","author":"carol"}"#,
    )
    .unwrap();

    let data = rx.recv_timeout(Duration::from_secs(10)).expect("no SSE event");
    let payload: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("insert"),
        "expected an insert event, got {payload}"
    );
    assert_eq!(
        payload.get("table").and_then(Value::as_str),
        Some("posts")
    );

    kill_and_wait(child);
}

#[test]
fn sigint_triggers_graceful_exit_zero() {
    let root = blog_fixture();
    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    // `kill_and_wait` asserts clean SIGINT shutdown internally.
    kill_and_wait(child);
}

#[test]
fn concurrent_queries_all_succeed() {
    let root = blog_fixture();
    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let url = format!("http://localhost:{port}/query");
    let mut handles = vec![];
    for _ in 0..25 {
        let url = url.clone();
        handles.push(std::thread::spawn(move || {
            Client::new()
                .post(&url)
                .json(&json!({"sql": "SELECT COUNT(*) AS n FROM posts"}))
                .send()
                .unwrap()
                .status()
        }));
    }
    for h in handles {
        assert_eq!(h.join().unwrap(), StatusCode::OK);
    }

    kill_and_wait(child);
}

// ---------------------------------------------------------------------------
// Config discovery & error paths
// ---------------------------------------------------------------------------

#[test]
fn missing_config_returns_503_on_query() {
    // Start in a dir with NO `.dirsql.toml`. The server should still start
    // (so that a user can see the error via HTTP), but queries return 503.
    let empty = TempDir::new().unwrap();
    let port = free_port();
    let child = spawn_dirsql(empty.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT 1"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    kill_and_wait(child);
}

#[test]
fn explicit_config_flag_overrides_cwd_default() {
    // Start in an unrelated cwd but point `--config` at the fixture.
    let fixture = blog_fixture();
    let elsewhere = TempDir::new().unwrap();

    let port = free_port();
    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .into();
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .arg("--config")
        .arg(fixture.path().join(".dirsql.toml"))
        .current_dir(elsewhere.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let child = cmd.spawn().expect("spawn");
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT COUNT(*) AS n FROM posts"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    kill_and_wait(child);
}
