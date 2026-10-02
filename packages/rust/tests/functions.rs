//! Integration tests for `[[dirsql.function]]` — config-declared SQL scalar
//! functions served by a lazy persistent worker process.
//!
//! These build a `DirSQL` from a real `.dirsql.toml` declaring functions
//! backed by trivial python3 worker scripts (newline-delimited JSON over
//! stdin/stdout), and assert the registration, protocol, lifecycle, and
//! timeout semantics through `db.query(...)`. They exercise the effectful
//! spawn path (kept out of colocated unit tests by the Rust isolation rule).
//!
//! Unix-only: the workers shell out to `python3`. The Rust CI test job runs
//! on Linux.
#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// Write a worker script whose body handles one decoded request (`args`) and
/// must assign the JSON-serializable response object to `resp`. The wrapper
/// does the line framing: read a line, decode `{"call": [...]}`, run `body`,
/// print `resp` as one line, flush, repeat.
fn write_worker(dir: &Path, filename: &str, body: &str) {
    let indented: String = body
        .lines()
        .map(|l| format!("        {l}\n"))
        .collect::<String>();
    let script = format!(
        r#"
import base64
import json
import os
import sys
import time

def main():
    for line in sys.stdin:
        req = json.loads(line)
        args = req["call"]
{indented}
        sys.stdout.write(json.dumps(resp, separators=(",", ":")) + "\n")
        sys.stdout.flush()

main()
"#
    );
    fs::write(dir.join(filename), script).unwrap();
}

fn build(root: &TempDir) -> Result<DirSQL, dirsql::DirSqlError> {
    DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
}

#[test]
fn declared_function_is_served_by_the_worker() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"ok": args[0].upper()}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT up('hello') AS v").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["v"], Value::Text("HELLO".into()));
}

#[test]
fn one_worker_serves_all_rows_never_one_process_per_row() {
    let root = TempDir::new().unwrap();
    let log = root.path().join("spawns.log");
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"ok": args[0].upper()}"#,
    );
    // Record one line per worker process start, before the request loop.
    let script = fs::read_to_string(root.path().join("worker.py")).unwrap();
    let script = script.replace(
        "def main():",
        &format!(
            "with open({log:?}, 'a') as f:\n    f.write('spawn\\n')\n\ndef main():",
            log = log.to_str().unwrap()
        ),
    );
    fs::write(root.path().join("worker.py"), script).unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "a").unwrap();
    fs::write(root.path().join("b.txt"), "b").unwrap();
    fs::write(root.path().join("c.txt"), "c").unwrap();

    let db = build(&root).unwrap();
    let rows = db
        .query("SELECT up(basename) AS v FROM './*.txt' ORDER BY v")
        .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["v"], Value::Text("A.TXT".into()));

    let spawns = fs::read_to_string(&log).unwrap();
    assert_eq!(
        spawns.lines().count(),
        1,
        "expected exactly one worker process for the whole invocation, got log: {spawns:?}"
    );
}

#[test]
fn worker_is_never_spawned_when_the_function_goes_uncalled() {
    let root = TempDir::new().unwrap();
    let log = root.path().join("spawns.log");
    write_worker(root.path(), "worker.py", r#"resp = {"ok": None}"#);
    let script = fs::read_to_string(root.path().join("worker.py")).unwrap();
    let script = script.replace(
        "def main():",
        &format!(
            "with open({log:?}, 'a') as f:\n    f.write('spawn\\n')\n\ndef main():",
            log = log.to_str().unwrap()
        ),
    );
    fs::write(root.path().join("worker.py"), script).unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT 1 AS one").unwrap();
    assert_eq!(rows.len(), 1);
    drop(db);

    assert!(
        !log.exists(),
        "worker must not spawn for a query that never calls the function"
    );
}

#[test]
fn worker_is_torn_down_when_the_index_is_dropped() {
    let root = TempDir::new().unwrap();
    write_worker(root.path(), "worker.py", r#"resp = {"ok": os.getpid()}"#);
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "pid"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT pid(0) AS pid").unwrap();
    let pid = match &rows[0]["pid"] {
        Value::Integer(pid) => *pid,
        other => panic!("expected an integer pid, got {other:?}"),
    };
    let alive = |pid: i64| {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("kill -0 {pid} 2>/dev/null"))
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(pid), "worker must be alive while the index lives");

    drop(db);

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if !alive(pid) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "worker (pid {pid}) still alive 10s after the index was dropped"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn arguments_are_encoded_as_json_per_protocol() {
    let root = TempDir::new().unwrap();
    // Echo back exactly what the worker received, so the query result carries
    // the wire encoding.
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"ok": json.dumps(args, separators=(",", ":"))}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "echo5"
args = [5]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db
        .query("SELECT echo5('txt', 1, 2.5, NULL, X'0102') AS v")
        .unwrap();
    assert_eq!(
        rows[0]["v"],
        Value::Text(r#"["txt",1,2.5,null,{"$bytes":"AQI="}]"#.into())
    );
}

#[test]
fn response_values_map_to_sql_types() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"kind = args[0]
if kind == "int":
    resp = {"ok": 7}
elif kind == "real":
    resp = {"ok": 2.5}
elif kind == "null":
    resp = {"ok": None}
elif kind == "blob":
    resp = {"ok": {"$bytes": base64.b64encode(b"\x01\x02").decode()}}
else:
    resp = {"ok": [1.5, 2.0]}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "typed"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let one =
        |kind: &str| db.query(&format!("SELECT typed('{kind}') AS v")).unwrap()[0]["v"].clone();
    assert_eq!(one("int"), Value::Integer(7));
    assert_eq!(one("real"), Value::Real(2.5));
    assert_eq!(one("null"), Value::Null);
    assert_eq!(one("blob"), Value::Blob(vec![1, 2]));
    // Arrays are bound as TEXT (JSON text) so sqlite-vec distance functions
    // accept them.
    assert_eq!(one("arr"), Value::Text("[1.5,2.0]".into()));
}

#[test]
fn worker_err_response_fails_the_query_with_the_message() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"err": "boom: cannot embed that"}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "boomer"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let err = db.query("SELECT boomer('x') AS v").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("boom: cannot embed that"), "got: {msg}");
}

#[test]
fn worker_crash_produces_an_actionable_error_not_a_hang() {
    let root = TempDir::new().unwrap();
    // Exit without replying: the first request must fail promptly with an
    // error naming the function, never hang.
    fs::write(
        root.path().join("worker.py"),
        "import sys\nsys.stdin.readline()\nsys.exit(1)\n",
    )
    .unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "crashy"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let started = Instant::now();
    let err = db.query("SELECT crashy('x') AS v").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("crashy"), "got: {msg}");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "crash must fail promptly, took {:?}",
        started.elapsed()
    );
}

#[test]
fn per_call_timeout_kills_the_call_with_an_actionable_error() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"time.sleep(30)
resp = {"ok": None}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "slow"
args = [1]
command = "python3 worker.py"
timeout = "1s"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let started = Instant::now();
    let err = db.query("SELECT slow('x') AS v").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("timed out"), "got: {msg}");
    assert!(msg.contains("slow"), "got: {msg}");
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "timeout must bound the call, took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_function_without_a_timeout_gets_the_thirty_second_default() {
    let root = TempDir::new().unwrap();
    // 2s is over any sub-second bound but comfortably under the 30s default:
    // the call must succeed, proving the default is the mechanism's own 30s.
    write_worker(
        root.path(),
        "worker.py",
        r#"time.sleep(2)
resp = {"ok": "done"}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "slowok"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT slowok('x') AS v").unwrap();
    assert_eq!(rows[0]["v"], Value::Text("done".into()));
}

#[test]
fn multi_arity_declared_functions_accept_each_arity_and_reject_others() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"if len(args) == 1:
    resp = {"ok": args[0].upper()}
else:
    resp = {"ok": args[0].upper() + ":" + str(args[1])}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1, 2]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT up('a') AS v").unwrap();
    assert_eq!(rows[0]["v"], Value::Text("A".into()));
    let rows = db.query("SELECT up('a', 'model-x') AS v").unwrap();
    assert_eq!(rows[0]["v"], Value::Text("A:model-x".into()));

    let err = db.query("SELECT up('a', 'b', 'c') AS v").unwrap_err();
    assert!(
        err.to_string().contains("wrong number of arguments"),
        "got: {err}"
    );
}

#[test]
fn undeclared_function_keeps_sqlites_no_such_function_error() {
    let root = TempDir::new().unwrap();
    write_worker(root.path(), "worker.py", r#"resp = {"ok": None}"#);
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let err = db.query("SELECT nope('a') AS v").unwrap_err();
    assert!(err.to_string().contains("no such function"), "got: {err}");
}

#[test]
fn duplicate_function_name_across_configs_errors_naming_both_sources() {
    let root = TempDir::new().unwrap();
    write_worker(root.path(), "worker.py", r#"resp = {"ok": None}"#);
    let a = root.path().join("a.dirsql.toml");
    let b = root.path().join("b.dirsql.toml");
    let entry = r#"
[[dirsql.function]]
name = "dup"
args = [1]
command = "python3 worker.py"
"#;
    fs::write(&a, entry).unwrap();
    fs::write(&b, entry).unwrap();

    let err = DirSQL::builder()
        .root(root.path())
        .config(&a)
        .config(&b)
        .build()
        .err()
        .expect("duplicate function names across configs must fail the build");
    let msg = err.to_string();
    assert!(msg.contains("dup"), "got: {msg}");
    assert!(msg.contains("a.dirsql.toml"), "got: {msg}");
    assert!(msg.contains("b.dirsql.toml"), "got: {msg}");
}

#[test]
fn function_entry_missing_command_is_an_actionable_config_error() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
"#,
    )
    .unwrap();

    let err = build(&root)
        .err()
        .expect("a [[dirsql.function]] entry without a command must fail the build");
    let msg = err.to_string();
    assert!(msg.contains("command"), "got: {msg}");
    assert!(msg.contains("[[dirsql.function]]"), "got: {msg}");
}

/// `meta` is progress metadata, not payload (dirsql#1034): the value bound into
/// the query is the `ok` field alone, whatever else rode along the line.
#[test]
fn a_response_carrying_cache_metadata_binds_only_its_ok_value() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"ok": args[0].upper(), "meta": {"cached": True}}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT up('hello') AS v").unwrap();
    assert_eq!(rows[0].get("v"), Some(&Value::Text("HELLO".to_string())));
}

/// A worker is free to send whatever `meta` it likes and the query must not
/// care: the field is advisory, so a shape core does not understand is ignored
/// rather than failing a query over a progress counter.
#[test]
fn an_unrecognized_meta_shape_does_not_fail_the_query() {
    let root = TempDir::new().unwrap();
    write_worker(
        root.path(),
        "worker.py",
        r#"resp = {"ok": args[0].upper(), "meta": "not-an-object"}"#,
    );
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
"#,
    )
    .unwrap();

    let db = build(&root).unwrap();
    let rows = db.query("SELECT up('hello') AS v").unwrap();
    assert_eq!(rows[0].get("v"), Some(&Value::Text("HELLO".to_string())));
}

/// Write a worker that speaks both protocol shapes: a `{"call": [...]}`
/// line is answered with one response, a `{"calls": [[...], ...]}` line with
/// `{"results": [<response>, ...]}`. `body` computes `resp` for one `args`
/// list and sees `batched` (whether it arrived in a batched request). Every
/// request line is appended verbatim to `log`.
fn write_batch_worker(dir: &Path, filename: &str, log: &Path, body: &str) {
    let indented: String = body
        .lines()
        .map(|l| format!("    {l}\n"))
        .collect::<String>();
    let script = format!(
        r#"
import json
import sys

def one(args, batched):
{indented}
    return resp

for line in sys.stdin:
    with open({log:?}, "a") as f:
        f.write(line)
    req = json.loads(line)
    if "calls" in req:
        resp = {{"results": [one(args, True) for args in req["calls"]]}}
    else:
        resp = one(req["call"], False)
    sys.stdout.write(json.dumps(resp, separators=(",", ":")) + "\n")
    sys.stdout.flush()
"#,
        log = log.to_str().unwrap()
    );
    fs::write(dir.join(filename), script).unwrap();
}

/// Every request the worker received, decoded, in order.
fn requests(log: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn write_batched_config(root: &Path, batch: &str) {
    fs::write(
        root.join(".dirsql.toml"),
        format!(
            r#"
[[dirsql.function]]
name = "up"
args = [1]
command = "python3 worker.py"
batch = {batch}
"#
        ),
    )
    .unwrap();
}

fn write_txt_files(root: &Path, names: &[&str]) {
    for name in names {
        fs::write(root.join(name), name).unwrap();
    }
}

/// With `batch` set, a statement that calls the function on every row pays
/// one round trip for the first value and then one batched request for the
/// rest, instead of one round trip per row. The rows come back exactly as
/// they would one call at a time.
#[test]
fn a_batched_function_collects_a_statements_values_into_one_request() {
    let root = TempDir::new().unwrap();
    let log = root.path().join("requests.log");
    write_batch_worker(
        root.path(),
        "worker.py",
        &log,
        r#"resp = {"ok": args[0].upper()}"#,
    );
    write_batched_config(root.path(), "8");
    write_txt_files(root.path(), &["a.txt", "b.txt", "c.txt"]);

    let db = build(&root).unwrap();
    let rows = db
        .query("SELECT up(basename) AS v FROM './*.txt' ORDER BY v")
        .unwrap();

    let values: Vec<&Value> = rows.iter().map(|r| &r["v"]).collect();
    assert_eq!(
        values,
        vec![
            &Value::Text("A.TXT".into()),
            &Value::Text("B.TXT".into()),
            &Value::Text("C.TXT".into())
        ]
    );
    let sent = requests(&log);
    assert_eq!(sent.len(), 2, "one call then one batch, got: {sent:?}");
    assert_eq!(sent[0]["call"].as_array().unwrap().len(), 1);
    let batched = sent[1]["calls"].as_array().unwrap();
    assert_eq!(batched.len(), 2, "the two other values ride one request");
    let mut seen: Vec<&str> = std::iter::once(&sent[0]["call"][0])
        .chain(batched.iter().map(|call| &call[0]))
        .map(|v| v.as_str().unwrap())
        .collect();
    seen.sort_unstable();
    assert_eq!(seen, ["a.txt", "b.txt", "c.txt"]);
}

/// `batch` caps the values per batched request: a corpus larger than the cap
/// is sent in consecutive full requests.
#[test]
fn batched_requests_are_split_at_the_batch_size() {
    let root = TempDir::new().unwrap();
    let log = root.path().join("requests.log");
    write_batch_worker(
        root.path(),
        "worker.py",
        &log,
        r#"resp = {"ok": args[0].upper()}"#,
    );
    write_batched_config(root.path(), "2");
    write_txt_files(root.path(), &["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"]);

    let db = build(&root).unwrap();
    let rows = db
        .query("SELECT up(basename) AS v FROM './*.txt' ORDER BY v")
        .unwrap();

    assert_eq!(rows.len(), 5);
    assert_eq!(rows[4]["v"], Value::Text("E.TXT".into()));
    let sent = requests(&log);
    let sizes: Vec<usize> = sent
        .iter()
        .map(|req| match req.get("calls") {
            Some(calls) => calls.as_array().unwrap().len(),
            None => req["call"].as_array().unwrap().len(),
        })
        .collect();
    assert_eq!(sizes, [1, 2, 2], "got: {sent:?}");
}

/// A per-entry `{"err": ...}` inside a batched response fails the query
/// with that message, exactly as the single-call protocol does.
#[test]
fn a_failed_entry_in_a_batched_response_fails_the_query() {
    let root = TempDir::new().unwrap();
    let log = root.path().join("requests.log");
    write_batch_worker(
        root.path(),
        "worker.py",
        &log,
        r#"resp = {"err": "boom: cannot upcase that"} if batched else {"ok": args[0]}"#,
    );
    write_batched_config(root.path(), "8");
    write_txt_files(root.path(), &["a.txt", "b.txt"]);

    let db = build(&root).unwrap();
    let err = db
        .query("SELECT up(basename) AS v FROM './*.txt' ORDER BY v")
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("boom: cannot upcase that"), "got: {msg}");
}

#[test]
fn batch_must_be_a_positive_integer() {
    for raw in ["0", "-4", "\"many\""] {
        let root = TempDir::new().unwrap();
        write_batched_config(root.path(), raw);
        let err = build(&root)
            .err()
            .unwrap_or_else(|| panic!("batch = {raw} must fail the build"));
        let msg = err.to_string();
        assert!(msg.contains("'batch'"), "batch = {raw}: {msg}");
        assert!(msg.contains("positive"), "batch = {raw}: {msg}");
        assert!(
            msg.contains("[[dirsql.function]] 'up'"),
            "batch = {raw}: {msg}"
        );
    }
}
