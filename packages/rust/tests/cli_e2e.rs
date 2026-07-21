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
/// The `.dirsql.toml` lives at the root; bare `dirsql` no longer auto-loads it
/// (#602), so callers pass it with `-c .dirsql.toml`.
/// `basename` is a filesystem-derived column, so rows are identified by their
/// file name (`Hello-World.json`) rather than by any path-derived value.
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
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
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
ddl = 'CREATE TABLE "posts" (basename TEXT)'
glob = "posts/*/*.json"
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

/// Spawn `dirsql` bound to `--port <port>` in `dir` with `extra` args
/// appended (e.g. `--persist`). Mirrors [`spawn_dirsql`] otherwise.
fn spawn_dirsql_with_args(dir: &std::path::Path, port: u16, extra: &[&str]) -> Child {
    let mut cmd: StdCommand = std::process::Command::cargo_bin("dirsql")
        .expect("`dirsql` binary must be built by `cargo test` with --features cli");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost");
    for a in extra {
        cmd.arg(a);
    }
    cmd.current_dir(dir)
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
        .stdout(predicates::str::contains("--port"))
        .stdout(predicates::str::contains("--persist"));
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
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![
            json!({"basename": "Hello-World.json"}),
            json!({"basename": "Second-Post.json"}),
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
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
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
    // Write into a post dir that already exists at startup so notify's
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
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
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
    // A parse-failing `-c` config degrades the server (still binds, queries
    // 503). A *missing* `-c` errors instead (see
    // `missing_explicit_config_exits_nonzero_naming_the_file`); no `-c` at all
    // serves the baked-in default (`no_config_serves_default_files_table`).
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "this is not valid toml [[[",
    )
    .unwrap();
    let port = free_port();
    let child = spawn_dirsql_with_args(dir.path(), port, &["-c", ".dirsql.toml"]);
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
    let child = spawn_dirsql_with_args(dir.path(), port, &["-c", ".dirsql.toml"]);
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

    let out = run_query_subcommand_with_config(dir.path(), "SELECT 1");
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
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(body, vec![json!({"basename": "Hello-World.json"})]);

    kill_and_wait(child);
}

#[test]
fn no_config_serves_path_tables_not_a_files_table() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();
    let port = free_port();
    let child = spawn_dirsql(dir.path(), port);
    wait_until_ready(port, Duration::from_secs(10));

    let client = Client::new();
    let url = format!("http://localhost:{port}/query");

    let miss = client
        .post(&url)
        .json(&json!({"sql": "SELECT basename FROM files"}))
        .send()
        .unwrap();
    assert_ne!(
        miss.status(),
        StatusCode::OK,
        "no `-c` must define no named tables"
    );
    let body = miss.text().unwrap();
    assert!(
        body.contains("no such table: files") && body.contains("did you mean FROM './'?"),
        "the no-config `files` miss must carry the path-table hint, got {body:?}"
    );

    let resp = client
        .post(&url)
        .json(&json!({"sql": "SELECT basename FROM './'"}))
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
        "expected the path-table to contain readme.md, got {names:?}"
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
        "echo \"SELECT basename FROM posts ORDER BY basename\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh to_sql.sh {args}"

[[table]]
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
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
            json!({"basename": "Hello-World.json"}),
            json!({"basename": "Second-Post.json"}),
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
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().unwrap();
    assert_eq!(
        body,
        json!({"results": [{"basename": "Hello-World.json"}, {"basename": "Second-Post.json"}]})
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
        "sleep 3\necho \"SELECT basename FROM posts ORDER BY basename\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh slow_to_sql.sh {args}"
hook-timeout = 1

[[table]]
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
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
        "sleep 2\necho \"SELECT basename FROM posts ORDER BY basename\"\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
pre-query = "sh slowish_to_sql.sh {args}"
hook-timeout = 60

[[table]]
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
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
            json!({"basename": "Hello-World.json"}),
            json!({"basename": "Second-Post.json"}),
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
ddl = "CREATE TABLE posts (basename TEXT, size INTEGER)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port, Duration::from_secs(10));

    let resp = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
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

/// Run `dirsql -c ./.dirsql.toml query <sql>` in `dir`. Bare `dirsql` no longer
/// auto-loads a cwd config (#602), so tests that expect the fixture's
/// `.dirsql.toml` to take effect pass it explicitly.
fn run_query_subcommand_with_config(dir: &std::path::Path, sql: &str) -> std::process::Output {
    std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg(sql)
        .arg("-c")
        .arg(".dirsql.toml")
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
    let sql = "SELECT basename FROM posts ORDER BY basename";

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml"]);
    wait_until_ready(port, Duration::from_secs(10));
    let http_body = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({ "sql": sql }))
        .send()
        .unwrap()
        .text()
        .unwrap();
    kill_and_wait(child);

    let out = run_query_subcommand_with_config(root.path(), sql);
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
fn query_subcommand_fans_out_file_to_overlapping_tables() {
    // Two plain tables with overlapping globs both match the one file; each
    // `dirsql query` returns the file's row (#580 fan-out).
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("data/2401.00001")).unwrap();
    fs::write(root.path().join("data/2401.00001/metadata.json"), "{}").unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE ta (path TEXT)"
glob = "data/*/metadata.json"

[[table]]
ddl = "CREATE TABLE tb (path TEXT)"
glob = "data/**/metadata.json"
"#,
    )
    .unwrap();

    for table in ["ta", "tb"] {
        let out =
            run_query_subcommand_with_config(root.path(), &format!("SELECT path FROM {table}"));
        assert!(
            out.status.success(),
            "`dirsql query` on {table} must succeed, got {out:?}"
        );
        let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
        let paths: Vec<&str> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["path"].as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["data/2401.00001/metadata.json"],
            "table {table} must contain the fanned-out file, got {paths:?}"
        );
    }
}

#[test]
fn bare_dirsql_ignores_cwd_config_and_serves_path_tables() {
    // #602: bare `dirsql` (no `-c`) no longer auto-loads a `./.dirsql.toml`
    // sitting in the invocation directory. With no config there are no named
    // tables at all, so the on-disk `posts` table is unreachable and
    // filesystem queries go through path-tables.
    let root = blog_fixture(); // writes a `.dirsql.toml` defining `posts`

    // Filesystem queries are served by path-tables...
    let files = run_query_subcommand(root.path(), "SELECT COUNT(*) AS n FROM './'");
    assert!(
        files.status.success(),
        "bare `dirsql query` must serve path-tables, got {files:?}"
    );

    // ...and the cwd config's `posts` table is NOT loaded.
    let posts = run_query_subcommand(root.path(), "SELECT COUNT(*) AS n FROM posts");
    assert!(
        !posts.status.success(),
        "bare `dirsql` must NOT auto-load ./.dirsql.toml, so `posts` must be \
         unknown, got {posts:?}"
    );
    let stderr = String::from_utf8(posts.stderr).unwrap();
    assert!(
        stderr.contains("posts"),
        "the error should name the missing `posts` table, got {stderr:?}"
    );
}

#[test]
fn missing_explicit_config_exits_nonzero_naming_the_file() {
    // #602: an explicit `-c` to a file that does not exist is an error — no
    // silent fallback to the baked-in default. The diagnostic names the file.
    let dir = TempDir::new().unwrap();
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg("SELECT 1")
        .arg("-c")
        .arg("./missing.toml")
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed");
    assert!(
        !out.status.success(),
        "a missing -c config must be a non-zero exit, got {out:?}"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("missing.toml"),
        "stderr must name the missing config file, got {stderr:?}"
    );
}

#[test]
fn config_flag_before_the_subcommand_is_a_hard_error() {
    // #609: config flags are subcommand-local. A `-c` placed BEFORE the
    // subcommand is rejected loudly (never silently dropped or straddled across
    // the subcommand boundary). Pass config AFTER the subcommand instead:
    // `dirsql query <sql> -c <cfg>`.
    let root = blog_fixture(); // valid `.dirsql.toml`, so the only failure is placement
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("-c")
        .arg(".dirsql.toml")
        .arg("query")
        .arg("SELECT 1")
        .current_dir(root.path())
        .output()
        .expect("spawning `dirsql query` failed");
    assert!(
        !out.status.success(),
        "a config flag before the subcommand must be a hard error, got {out:?}"
    );
    let stderr = String::from_utf8(out.stderr).unwrap().to_lowercase();
    assert!(
        stderr.contains("subcommand") || stderr.contains("cannot be used"),
        "the error should explain the flag conflicts with the subcommand, got {stderr:?}"
    );
}

/// Run `dirsql --include-default [-c <cfg>]... query <sql>` in `dir`.
fn run_query_include_default(
    dir: &std::path::Path,
    configs: &[&str],
    sql: &str,
) -> std::process::Output {
    let mut cmd = std::process::Command::cargo_bin("dirsql").expect("binary must exist");
    // Config flags are subcommand-local (#609): pass them AFTER `query <sql>`.
    cmd.arg("query").arg(sql).arg("--include-default");
    for cfg in configs {
        cmd.arg("-c").arg(cfg);
    }
    cmd.current_dir(dir)
        .output()
        .expect("spawning `dirsql query` failed")
}

#[test]
fn include_default_composes_baked_in_files_with_an_explicit_config() {
    // #604: the hidden `--include-default` flag seeds the baked-in default
    // `files` table BEFORE the explicit `-c` configs, so a config no longer
    // suppresses the default. This is the additive composition the plugin
    // launcher (#529) injects for row 2 (no user `-c` + plugin): the result is
    // the baked-in default PLUS the config's own tables.
    let root = blog_fixture(); // `.dirsql.toml` defines `posts`

    let files = run_query_include_default(
        root.path(),
        &[".dirsql.toml"],
        "SELECT COUNT(*) AS n FROM files",
    );
    assert!(
        files.status.success(),
        "the baked-in `files` table must be present under --include-default, got {files:?}"
    );

    let posts = run_query_include_default(
        root.path(),
        &[".dirsql.toml"],
        "SELECT basename FROM posts ORDER BY basename",
    );
    assert!(
        posts.status.success(),
        "the explicit config's `posts` table must ALSO be present, got {posts:?}"
    );
    let rows: Value = serde_json::from_slice(&posts.stdout).unwrap();
    let basenames: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["basename"].as_str())
        .collect();
    assert_eq!(
        basenames,
        vec!["Hello-World.json", "Second-Post.json"],
        "the config's posts must load alongside the default files table, got {basenames:?}"
    );
}

#[test]
fn include_default_with_no_config_serves_the_bare_default() {
    // #604: `--include-default` with no `-c` is idempotent — it is exactly the
    // bare baked-in default (row 1). Seeding the default and then merging an
    // empty config set changes nothing.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();

    let out = run_query_include_default(dir.path(), &[], "SELECT basename FROM files");
    assert!(
        out.status.success(),
        "`--include-default` alone must serve the default files table, got {out:?}"
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
fn include_default_conflicting_files_table_exits_nonzero_naming_files() {
    // #604: seeding the baked-in `files` table and then loading a `-c` config
    // that ALSO defines `files` is a duplicate-table conflict, caught by the
    // existing dedup (no new conflict machinery). The diagnostic names the
    // duplicated table.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("dup.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (x TEXT)"
glob = "**/*"
"#,
    )
    .unwrap();

    let out = run_query_include_default(dir.path(), &["dup.toml"], "SELECT 1");
    assert!(
        !out.status.success(),
        "a config redefining `files` under --include-default must conflict, got {out:?}"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("files") && stderr.to_lowercase().contains("duplicate"),
        "the conflict must name the duplicate `files` table, got {stderr:?}"
    );
}

#[test]
fn explicit_config_without_include_default_suppresses_the_baked_in_files() {
    // #604 row 3: an explicit `-c` WITHOUT `--include-default` keeps the
    // replacement semantics of #602 — the baked-in default is suppressed, so
    // only the config's own tables exist. This is what makes --include-default
    // meaningful (it opts the default back IN) and pins the flag's condition.
    let root = blog_fixture(); // `.dirsql.toml` defines `posts`, never `files`

    let posts = run_query_subcommand_with_config(root.path(), "SELECT COUNT(*) AS n FROM posts");
    assert!(
        posts.status.success(),
        "the explicit config's `posts` table must load, got {posts:?}"
    );

    let files = run_query_subcommand_with_config(root.path(), "SELECT COUNT(*) AS n FROM files");
    assert!(
        !files.status.success(),
        "an explicit `-c` without --include-default must suppress the baked-in \
         `files` table, got {files:?}"
    );
    let stderr = String::from_utf8(files.stderr).unwrap();
    assert!(
        stderr.contains("files"),
        "the error should name the absent `files` table, got {stderr:?}"
    );
}

#[test]
fn include_default_is_hidden_from_help() {
    // #604: `--include-default` is internal launcher plumbing, not a public
    // flag — it must not appear in `--help`.
    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("--help")
        .output()
        .expect("spawning `dirsql --help` failed");
    assert!(out.status.success(), "`--help` must exit 0, got {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("--include-default"),
        "the internal --include-default flag must be hidden from --help, got:\n{stdout}"
    );
}

#[test]
fn init_output_loads_when_passed_explicitly_with_config_flag() {
    // #602: `dirsql init` remains a scaffold, but its output no longer
    // auto-loads — you pass it explicitly with `-c`. Write the starter, then
    // load it via `-c` and confirm it serves the `files` table it declares.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();

    let init = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql init` failed");
    assert!(
        init.status.success(),
        "`dirsql init` must succeed, got {init:?}"
    );

    let out = std::process::Command::cargo_bin("dirsql")
        .expect("binary must exist")
        .arg("query")
        .arg("SELECT basename FROM files")
        .arg("-c")
        .arg(".dirsql.toml")
        .current_dir(dir.path())
        .output()
        .expect("spawning `dirsql query` failed");
    assert!(
        out.status.success(),
        "the init-written config must load via `-c`, got {out:?}"
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
        "the init config's files table must contain readme.md, got {names:?}"
    );
}

#[test]
fn query_subcommand_without_config_hints_at_the_path_table_form() {
    // Config discovery matches server mode: no `.dirsql.toml` means no named
    // tables at all. `files` is a miss, and carries the path-table hint.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("readme.md"), "hello").unwrap();

    let miss = run_query_subcommand(dir.path(), "SELECT basename FROM files");
    assert!(
        !miss.status.success(),
        "`dirsql query` must not serve an implicit files table, got {miss:?}"
    );
    let stderr = String::from_utf8(miss.stderr).unwrap();
    assert!(
        stderr.contains("no such table: files") && stderr.contains("did you mean FROM './'?"),
        "the no-config `files` miss must carry the path-table hint, got {stderr:?}"
    );

    let out = run_query_subcommand(dir.path(), "SELECT basename FROM './'");
    assert!(
        out.status.success(),
        "`dirsql query` must serve path-tables with no config, got {out:?}"
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
        "expected the path-table to contain readme.md, got {names:?}"
    );
}

#[test]
fn root_config_key_degrades_server_with_503_naming_the_key() {
    // `root` is no longer a config key (#540): the runner owns the index root.
    // An old config carrying it is a hard config error, so the server degrades
    // and `POST /query` returns 503 whose diagnostic names `root`.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "[dirsql]\nroot = \"docs\"\n",
    )
    .unwrap();
    let port = free_port();
    let child = spawn_dirsql_with_args(dir.path(), port, &["-c", ".dirsql.toml"]);
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
        error.contains("root"),
        "503 diagnostic must name the unknown key, got {error:?}"
    );

    kill_and_wait(child);
}

#[test]
fn config_elsewhere_indexes_invocation_cwd_not_config_parent() {
    // With `root` gone (#540), `--config /elsewhere/.dirsql.toml` roots at the
    // invocation cwd, not the config's parent. The data lives in the cwd; the
    // config's own directory holds nothing to index.
    let cwd = TempDir::new().unwrap();
    fs::create_dir_all(cwd.path().join("posts/alice")).unwrap();
    fs::create_dir_all(cwd.path().join("posts/bob")).unwrap();
    fs::write(cwd.path().join("posts/alice/Hello-World.json"), "{}").unwrap();
    fs::write(cwd.path().join("posts/bob/Second-Post.json"), "{}").unwrap();

    let elsewhere = TempDir::new().unwrap();
    fs::write(
        elsewhere.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (basename TEXT)"
glob = "posts/*/*.json"
"#,
    )
    .unwrap();

    let port = free_port();
    let mut cmd: StdCommand =
        std::process::Command::cargo_bin("dirsql").expect("binary must exist");
    cmd.arg("--port")
        .arg(port.to_string())
        .arg("--host")
        .arg("localhost")
        .arg("--config")
        .arg(elsewhere.path().join(".dirsql.toml"))
        .current_dir(cwd.path())
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
    let body: Vec<Value> = resp.json().unwrap();
    assert_eq!(
        body,
        vec![json!({"n": 2})],
        "posts must be indexed from the invocation cwd, not the config's parent"
    );

    kill_and_wait(child);
}

#[test]
fn explicit_config_flag_loads_the_named_config() {
    // An explicit `--config` loads that file regardless of the cwd (there is
    // no cwd `.dirsql.toml` auto-detection to override, #602).
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
fn short_config_flag_loads_the_named_config() {
    // Same as the long form: `-c` loads the named config regardless of cwd.
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

// ---------------------------------------------------------------------------
// Persistence: the `--persist [PATH]` flag (#549)
// ---------------------------------------------------------------------------

#[test]
fn persist_config_key_degrades_server_with_503_naming_the_key() {
    // Persistence moved to the `--persist` flag: `persist` is no longer a TOML
    // key, so a config carrying it is a hard load error (#536). The server
    // degrades and `POST /query` returns 503 whose diagnostic names `persist`.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".dirsql.toml"),
        "[dirsql]\npersist = true\n",
    )
    .unwrap();
    let port = free_port();
    let child = spawn_dirsql_with_args(dir.path(), port, &["-c", ".dirsql.toml"]);
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
        error.contains("persist"),
        "503 diagnostic must name the unknown `persist` key, got {error:?}"
    );

    kill_and_wait(child);
}

#[test]
fn persist_flag_writes_default_cache_and_restart_serves() {
    // Bare `--persist` writes the cache at the default `<root>/.dirsql/cache.db`
    // during the startup scan; a restart with `--persist` reopens that cache
    // (trusting unchanged files) and serves the same rows.
    let root = blog_fixture();
    let cache = root.path().join(".dirsql").join("cache.db");

    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml", "--persist"]);
    wait_until_ready(port, Duration::from_secs(10));
    let first = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .unwrap()
        .text()
        .unwrap();
    kill_and_wait(child);

    assert!(
        cache.exists(),
        "bare --persist must write the default cache at {}",
        cache.display()
    );

    // Restart against the unchanged tree: the cache is reused and the same
    // rows are served.
    let port = free_port();
    let child = spawn_dirsql_with_args(root.path(), port, &["-c", ".dirsql.toml", "--persist"]);
    wait_until_ready(port, Duration::from_secs(10));
    let second = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT basename FROM posts ORDER BY basename"}))
        .send()
        .unwrap()
        .text()
        .unwrap();
    kill_and_wait(child);

    assert_eq!(
        first, second,
        "a persisted restart must serve the same rows"
    );
}

#[test]
fn persist_flag_with_path_writes_the_cache_there() {
    // `--persist <path>` writes the cache at the given path, not the default.
    let root = blog_fixture();
    let cache_dir = TempDir::new().unwrap();
    let cache = cache_dir.path().join("nested").join("x.db");

    let port = free_port();
    let child = spawn_dirsql_with_args(
        root.path(),
        port,
        &["-c", ".dirsql.toml", "--persist", cache.to_str().unwrap()],
    );
    wait_until_ready(port, Duration::from_secs(10));
    let resp = Client::new()
        .post(format!("http://localhost:{port}/query"))
        .json(&json!({"sql": "SELECT COUNT(*) AS n FROM posts"}))
        .send()
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    kill_and_wait(child);

    assert!(
        cache.exists(),
        "--persist <path> must write the cache at {}",
        cache.display()
    );
    assert!(
        !root.path().join(".dirsql").join("cache.db").exists(),
        "the default cache must not be written when a path is given"
    );
}
