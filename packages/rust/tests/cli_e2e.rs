//! End-to-end tests for the `dirsql` CLI binary.
//!
//! These tests spawn the actual compiled `dirsql` binary as a subprocess,
//! talk to it over real HTTP, and drive real filesystem mutations. Nothing
//! is mocked. Tests are deliberately tolerant of startup / shutdown timing
//! via bounded retries; they are NOT tolerant of missing or broken
//! behavior described in `docs/reference/cli.md`.
//!
//! Gated behind `--features cli`: the `dirsql` bin target itself is
//! `required-features = ["cli"]`, so without the feature there's no
//! binary for `assert_cmd::cargo_bin` to find. Compile to an empty
//! test binary in that case so default `cargo test` still succeeds.

#![cfg(feature = "cli")]

use std::fs;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::process::{Child, Command as StdCommand, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::prelude::*;
use reqwest::{StatusCode, blocking::Client};
use serde_json::{Value, json};
use tempfile::TempDir;

/// Write a two-post blog fixture into a fresh tempdir and return it.
/// The `.dirsql.toml` lives at the root so `dirsql` can discover it.
/// `title` and `author` are captured from the file path
/// (`posts/{author}/{title}.json`).
fn blog_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts/alice")).unwrap();
    fs::create_dir_all(root.path().join("posts/bob")).unwrap();
    fs::write(root.path().join("posts/alice/Hello-World.json"), "{}").unwrap();
    fs::write(root.path().join("posts/bob/Second-Post.json"), "{}").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();
    root
}

/// A blog fixture whose `.dirsql.toml` names the table with a **quoted**
/// identifier (`"posts"`) -- the canonical DDL shape emitted by ORMs / schema
/// tools.
fn quoted_blog_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts/alice")).unwrap();
    fs::write(root.path().join("posts/alice/Hello-World.json"), "{}").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        // Single-quoted TOML string so the embedded double quotes are literal.
        r#"
[[table]]
ddl = 'CREATE TABLE "posts" (title TEXT, author TEXT, basename TEXT)'
glob = "posts/{author}/{title}.json"
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
        .expect("`dirsql` binary must be built by `cargo test` with --features cli");
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
    let client = Client::builder()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap();
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
    // Every flag documented in docs/reference/cli.md must appear in `--help`.
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("-c, --config"))
        .stdout(predicates::str::contains("--host"))
        .stdout(predicates::str::contains("--port"));
}

#[test]
fn server_announces_bind_on_stdout() {
    let root = blog_fixture();
    let port = free_port();
    let mut child = spawn_dirsql(root.path(), port);

    let stdout = child.stdout.take().expect("stdout piped");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("expected a startup line");
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
            json!({"title": "Hello-World"}),
            json!({"title": "Second-Post"}),
        ]
    );

    kill_and_wait(child);
}

#[test]
fn post_query_rejects_read_of_internal_bookkeeping_table() {
    let root = blog_fixture();
    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT * FROM _dirsql_internal_rows"}))
        .send()
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "reading an internal table must be rejected, not served"
    );
    let body: Value = resp.json().unwrap();
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.to_lowercase().contains("not authorized"),
        "400 body should explain the read is not authorized, got {error:?}"
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
    // Write into an author dir that already exists at startup so notify's
    // watch is guaranteed to be installed. Creating a new dir + writing
    // immediately races inotify's recursive-watch installation; that race
    // is observable and flaky, not a feature under test here.
    fs::write(root.path().join("posts/alice/Brand-New-Post.json"), "{}").unwrap();

    let data = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("no SSE event");
    let payload: Value = serde_json::from_str(&data).unwrap();
    assert_eq!(
        payload.get("action").and_then(Value::as_str),
        Some("insert"),
        "expected an insert event, got {payload}"
    );
    assert_eq!(payload.get("table").and_then(Value::as_str), Some("posts"));

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

#[test]
fn unloadable_config_returns_503_on_query() {
    // A parse-failing config degrades the server (still binds, queries 503);
    // a *missing* config does not -- see `no_config_serves_default_files_table`.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "this is not valid toml [[[",
    )
    .unwrap();
    let port = free_port();
    let child = spawn_dirsql(dir.path(), port);
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
fn unknown_config_key_degrades_server_with_503_naming_the_key() {
    // A misspelled key is a hard config error (#536): the server degrades and
    // `POST /query` returns 503 whose diagnostic names the offending key.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "[dirsql]\npersistpath = \"cache.db\"\n",
    )
    .unwrap();
    let port = free_port();
    let child = spawn_dirsql(dir.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT 1"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error = resp.json::<Value>().unwrap()["error"]
        .as_str()
        .expect("503 body carries an `error` string")
        .to_string();
    assert!(
        error.contains("persistpath"),
        "503 diagnostic must name the unknown key, got {error:?}"
    );

    kill_and_wait(child);
}

#[test]
fn query_subcommand_rejects_unknown_config_key_with_nonzero_exit() {
    // Same strict-config diagnostic on the one-shot surface (#536): `dirsql
    // query` exits non-zero and names the unknown key on stderr.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "[dirsql]\npersistpath = \"cache.db\"\n",
    )
    .unwrap();

    let out = run_query_subcommand(dir.path(), "SELECT 1");
    assert!(
        !out.status.success(),
        "an unknown config key must be a non-zero exit, got {out:?}"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("persistpath"),
        "stderr must name the unknown key, got {stderr:?}"
    );
}

#[test]
fn quoted_identifier_table_in_toml_is_served_over_http() {
    // The quoted DDL identifier resolves to the bare table name `posts`.
    let root = quoted_blog_fixture();
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
    assert_eq!(body, vec![json!({"title": "Hello-World"})]);

    kill_and_wait(child);
}

#[test]
fn no_config_serves_default_files_table() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();
    let port = free_port();
    let child = spawn_dirsql(dir.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM files"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let rows: Value = resp.json().unwrap();
    let names: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["basename"].as_str())
        .collect();
    assert!(
        names.contains(&"readme.md"),
        "expected `files` table to contain readme.md, got {names:?}"
    );

    kill_and_wait(child);
}

#[cfg(unix)]
#[test]
fn pre_query_hook_rewrites_body_into_sql_over_http() {
    // The passthrough path would 400 on this non-JSON body, so rows coming
    // back proves the hook ran. The script is referenced by a bare relative
    // name to exercise cwd = the config file's directory.
    let root = blog_fixture();
    fs::write(
        root.path().join("to_sql.sh"),
        "echo \"SELECT title FROM posts ORDER BY title\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh to_sql.sh {args}"

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .body("please give me the posts")
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![
            json!({"title": "Hello-World"}),
            json!({"title": "Second-Post"}),
        ]
    );

    kill_and_wait(child);
}

#[cfg(unix)]
#[test]
fn post_query_hook_reshapes_response_over_http() {
    // The `{results: …}` envelope coming back proves the hook reshaped the
    // response. The script is referenced by a bare relative name to exercise
    // cwd = the config file's directory.
    let root = blog_fixture();
    fs::write(
        root.path().join("wrap.sh"),
        "data=$(cat)\necho \"{\\\"results\\\": $data}\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
post-query = "sh wrap.sh {args}"

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT title FROM posts ORDER BY title"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().unwrap();
    assert_eq!(
        body,
        json!({"results": [{"title": "Hello-World"}, {"title": "Second-Post"}]})
    );

    kill_and_wait(child);
}

#[cfg(unix)]
#[test]
fn pre_query_hook_exceeding_configured_timeout_returns_500() {
    // Under the default 30s timeout this hook would finish; the 500 proves
    // the configured 1-second `hook-timeout` applied.
    let root = blog_fixture();
    fs::write(
        root.path().join("slow_to_sql.sh"),
        "sleep 3\necho \"SELECT title FROM posts ORDER BY title\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh slow_to_sql.sh {args}"
hook-timeout = 1

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap()
        .post(format!("http://localhost:{port}/query"))
        .body("please give me the posts")
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = resp.json().unwrap();
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("timed out"),
        "500 body should describe the timeout, got {error:?}"
    );

    kill_and_wait(child);
}

#[cfg(unix)]
#[test]
fn pre_query_hook_within_generous_configured_timeout_succeeds() {
    // Proves `hook-timeout` is read as seconds: a 60 read as milliseconds
    // would kill this 2-second hook.
    let root = blog_fixture();
    fs::write(
        root.path().join("slowish_to_sql.sh"),
        "sleep 2\necho \"SELECT title FROM posts ORDER BY title\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh slowish_to_sql.sh {args}"
hook-timeout = 60

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap()
        .post(format!("http://localhost:{port}/query"))
        .body("please give me the posts")
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![
            json!({"title": "Hello-World"}),
            json!({"title": "Second-Post"}),
        ]
    );

    kill_and_wait(child);
}

#[cfg(unix)]
#[test]
fn post_query_hook_exceeding_configured_timeout_returns_500() {
    let root = blog_fixture();
    fs::write(
        root.path().join("slow_wrap.sh"),
        "data=$(cat)\nsleep 3\necho \"{\\\"results\\\": $data}\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
post-query = "sh slow_wrap.sh {args}"
hook-timeout = 1

[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT, basename TEXT, size INTEGER)"
glob = "posts/{author}/{title}.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT title FROM posts ORDER BY title"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = resp.json().unwrap();
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("timed out"),
        "500 body should describe the timeout, got {error:?}"
    );

    kill_and_wait(child);
}

// ---------------------------------------------------------------------------
// One-shot `dirsql query` subcommand (#399 / #439)
// ---------------------------------------------------------------------------

/// Run `dirsql query <sql>` in `dir` and return the completed output.
fn run_query_subcommand(dir: &std::path::Path, sql: &str) -> std::process::Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .current_dir(dir)
        .output()
        .expect("spawning `dirsql query` failed")
}

#[test]
fn query_subcommand_stdout_is_byte_identical_to_the_http_response() {
    // #439 parity: the same SQL over the same fixture through both surfaces
    // must yield identical bytes — the subcommand is a thin adapter over the
    // same execute_query pipeline the server uses, so stdout IS the HTTP body.
    let root = blog_fixture();
    let sql = "SELECT title FROM posts ORDER BY title";

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));
    let http_body = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({ "sql": sql }))
        .send()
        .unwrap()
        .text()
        .unwrap();
    kill_and_wait(child);

    let out = run_query_subcommand(root.path(), sql);
    assert!(
        out.status.success(),
        "`dirsql query` must exit 0 on success, got {out:?}"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.trim_end_matches('\n'),
        http_body,
        "CLI stdout must be byte-identical to the HTTP response body"
    );
}

#[test]
fn query_subcommand_rejects_internal_table_read_with_the_http_error_message() {
    // #439 parity on the error path: a rejected read (#378 internal-table
    // denial) is a non-zero exit carrying the SAME diagnostic the HTTP
    // surface returns in its `{"error": …}` body — an error, never empty
    // output.
    let root = blog_fixture();
    let sql = "SELECT * FROM _dirsql_internal_rows";

    let port = free_port();
    let child = spawn_dirsql(root.path(), port);
    wait_until_ready(port, Duration::from_secs(10));
    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({ "sql": sql }))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let http_error = resp.json::<Value>().unwrap()["error"]
        .as_str()
        .expect("HTTP error body carries an `error` string")
        .to_string();
    kill_and_wait(child);

    let out = run_query_subcommand(root.path(), sql);
    assert!(
        !out.status.success(),
        "a rejected read must be a non-zero exit, got {out:?}"
    );
    assert!(
        out.stdout.is_empty(),
        "a rejected read must not produce stdout rows, got {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains(&http_error),
        "stderr must carry the same diagnostic the HTTP surface returns \
         ({http_error:?}), got {stderr:?}"
    );
}

#[test]
fn query_subcommand_rejects_blank_sql_with_nonzero_exit() {
    // The subcommand synthesizes the same `{"sql": …}` intake the server
    // parses, so blank SQL hits the shared empty-rejection with the same
    // message the HTTP 400 body carries.
    let root = blog_fixture();
    let out = run_query_subcommand(root.path(), "   ");
    assert!(
        !out.status.success(),
        "blank SQL must be a non-zero exit, got {out:?}"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("`sql` must not be empty"),
        "stderr must carry the shared intake message, got {stderr:?}"
    );
}

#[test]
fn query_subcommand_serves_default_files_table_without_config() {
    // Config discovery matches server mode: no `.dirsql.toml` means the
    // default `files` table, queryable out of the box.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();

    let out = run_query_subcommand(dir.path(), "SELECT basename FROM files");
    assert!(
        out.status.success(),
        "`dirsql query` must serve the default files table, got {out:?}"
    );
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["basename"].as_str())
        .collect();
    assert!(
        names.contains(&"readme.md"),
        "expected the default files table to contain readme.md, got {names:?}"
    );
}

#[test]
fn explicit_config_flag_overrides_cwd_default() {
    let fixture = blog_fixture();
    let elsewhere = TempDir::new().unwrap();

    let port = free_port();
    let mut cmd: StdCommand =
        std::process::Command::cargo_bin("dirsql").expect("binary must exist");
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

#[test]
fn short_config_flag_overrides_cwd_default() {
    let fixture = blog_fixture();
    let elsewhere = TempDir::new().unwrap();

    let port = free_port();
    let mut cmd: StdCommand =
        std::process::Command::cargo_bin("dirsql").expect("binary must exist");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .arg("-c")
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
