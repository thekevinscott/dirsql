//! E2E tests: spawn the actual `dirsql` binary and talk to it over HTTP.

use assert_cmd::cargo::CommandCargoExt;
use std::io::Write;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn pick_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

fn write_fixture(dir: &std::path::Path, include_data: bool) {
    let toml = r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT, qty INTEGER)"
glob = "data.jsonl"
format = "jsonl"
"#;
    std::fs::write(dir.join(".dirsql.toml"), toml).unwrap();
    if include_data {
        let mut f = std::fs::File::create(dir.join("data.jsonl")).unwrap();
        writeln!(f, "{{\"name\":\"apple\",\"qty\":3}}").unwrap();
        writeln!(f, "{{\"name\":\"pear\",\"qty\":5}}").unwrap();
    }
}

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn wait_ready(&self) -> bool {
        let start = Instant::now();
        let url = format!("http://127.0.0.1:{}/healthz", self.port);
        while start.elapsed() < Duration::from_secs(5) {
            if let Ok(r) = reqwest::blocking::get(&url)
                && r.status().is_success()
            {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(args: &[&str], cwd: Option<&std::path::Path>, port: u16) -> Server {
    let mut cmd = Command::cargo_bin("dirsql").unwrap();
    cmd.args(args);
    cmd.arg("--port").arg(port.to_string());
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let child = cmd.spawn().expect("spawn dirsql");
    Server { child, port }
}

fn post_query(port: u16, sql: &str) -> (u16, serde_json::Value) {
    let url = format!("http://127.0.0.1:{port}/query");
    let resp = reqwest::blocking::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "sql": sql }))
        .send()
        .unwrap();
    let status = resp.status().as_u16();
    let body: serde_json::Value = resp.json().unwrap_or(serde_json::Value::Null);
    (status, body)
}

#[test]
fn no_args_defaults_to_cwd_and_dirsql_toml() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let server = spawn(&[], Some(tmp.path()), port);
    assert!(server.wait_ready(), "server never became ready");

    let (status, body) = post_query(port, "SELECT name, qty FROM items ORDER BY name");
    assert_eq!(status, 200);
    let rows = body.get("rows").unwrap().as_array().unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn explicit_dir_argument() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let dir = tmp.path().to_str().unwrap();
    let server = spawn(&[dir], None, port);
    assert!(server.wait_ready());

    let (status, body) = post_query(port, "SELECT COUNT(*) AS c FROM items");
    assert_eq!(status, 200);
    let rows = body["rows"].as_array().unwrap();
    assert_eq!(rows[0]["c"], serde_json::json!(2));
}

#[test]
fn explicit_config_flag_overrides_default() {
    let data_dir = TempDir::new().unwrap();
    let conf_dir = TempDir::new().unwrap();

    // data dir has the jsonl file, conf dir has the toml
    let mut f = std::fs::File::create(data_dir.path().join("data.jsonl")).unwrap();
    writeln!(f, "{{\"name\":\"k\",\"qty\":1}}").unwrap();
    let toml = r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT, qty INTEGER)"
glob = "data.jsonl"
format = "jsonl"
"#;
    let conf_path = conf_dir.path().join("custom.toml");
    std::fs::write(&conf_path, toml).unwrap();

    let port = pick_port();
    let dir = data_dir.path().to_str().unwrap();
    let conf = conf_path.to_str().unwrap();
    let server = spawn(&[dir, "--config", conf], None, port);
    assert!(server.wait_ready());

    let (status, body) = post_query(port, "SELECT COUNT(*) AS c FROM items");
    assert_eq!(status, 200);
    assert_eq!(body["rows"][0]["c"], serde_json::json!(1));
}

#[test]
fn port_flag_honored() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let server = spawn(&[], Some(tmp.path()), port);
    assert!(server.wait_ready());
    // implicitly: if wait_ready succeeded on that port, the flag worked.
    let url = format!("http://127.0.0.1:{port}/healthz");
    let r = reqwest::blocking::get(&url).unwrap();
    assert!(r.status().is_success());
}

#[test]
fn malformed_sql_returns_400_with_error_body() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let server = spawn(&[], Some(tmp.path()), port);
    assert!(server.wait_ready());

    let (status, body) = post_query(port, "NOT VALID SQL ;;");
    assert_eq!(status, 400);
    assert!(body.get("error").is_some());
}

#[test]
fn missing_config_file_exits_nonzero_with_clear_message() {
    let tmp = TempDir::new().unwrap();
    // no .dirsql.toml at all
    let port = pick_port();
    let mut cmd = Command::cargo_bin("dirsql").unwrap();
    cmd.current_dir(tmp.path());
    cmd.arg("--port").arg(port.to_string());
    let output = cmd.output().expect("run dirsql");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("config") || stderr.to_lowercase().contains(".dirsql.toml"),
        "stderr did not mention config: {stderr}"
    );
}

#[test]
fn events_route_returns_501_for_now() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let server = spawn(&[], Some(tmp.path()), port);
    assert!(server.wait_ready());

    let url = format!("http://127.0.0.1:{port}/events");
    let r = reqwest::blocking::get(&url).unwrap();
    assert_eq!(r.status().as_u16(), 501);
}

#[test]
fn healthz_returns_200() {
    let tmp = TempDir::new().unwrap();
    write_fixture(tmp.path(), true);
    let port = pick_port();
    let server = spawn(&[], Some(tmp.path()), port);
    assert!(server.wait_ready());
    let url = format!("http://127.0.0.1:{port}/healthz");
    let r = reqwest::blocking::get(&url).unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.text().unwrap(), "ok");
}
