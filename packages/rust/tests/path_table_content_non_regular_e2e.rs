//! CLI e2e: a file the scan finds as a regular file but that a worker
//! function replaces with a non-regular entry before the same statement reads
//! its content yields NULL content and the query returns. The real `dirsql`
//! binary, a real python worker, a real temp tree, nothing mocked.

#![cfg(feature = "cli")]

use std::fs;
use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Long enough for any read of a one-byte file; a blocked open never ends.
const QUERY_BOUND: Duration = Duration::from_secs(60);

const WORKER: &str = r#"
import json
import os
import sys

for line in sys.stdin:
    path, kind, _ = json.loads(line)["call"]
    os.remove(path)
    if kind == "fifo":
        os.mkfifo(path)
    elif kind == "device":
        os.symlink(r"\\.\NUL" if os.name == "nt" else "/dev/null", path)
    else:
        os.mkdir(path)
    sys.stdout.write(json.dumps({"ok": kind}) + "\n")
    sys.stdout.flush()
"#;

/// The scan is materialized before `swap` runs, and the content subquery is
/// correlated with its result, so the read follows the swap.
const QUERY: &str = "WITH n AS MATERIALIZED (SELECT count(*) AS c FROM './d/*.txt'), \
     s AS MATERIALIZED (SELECT swap('d/b.txt', '{kind}', c) AS done FROM n) \
     SELECT s.done AS done, \
     (SELECT content FROM './d/*.txt' WHERE path = 'd/b.txt' AND s.done IS NOT NULL) AS content \
     FROM s";

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

fn kinds() -> Vec<&'static str> {
    let mut kinds = vec!["directory", "device"];
    if cfg!(unix) {
        kinds.push("fifo");
    }
    kinds
}

fn run(kind: &str) -> Output {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    fs::write(root.path().join("d/a.txt"), "a").unwrap();
    fs::write(root.path().join("d/b.txt"), "b").unwrap();
    fs::write(root.path().join("worker.py"), WORKER).unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            "[[dirsql.function]]\nname = \"swap\"\nargs = [3]\ncommand = \"{} worker.py\"\n",
            python()
        ),
    )
    .unwrap();
    Command::cargo_bin("dirsql")
        .unwrap()
        .arg("query")
        .arg(QUERY.replace("{kind}", kind))
        .arg("-c")
        .arg(root.path().join(".dirsql.toml"))
        .current_dir(root.path())
        .timeout(QUERY_BOUND)
        .output()
        .unwrap()
}

/// What the query printed for `kind`: the swap's result and `b.txt`'s
/// content, or the failure when it did not succeed.
fn outcome(kind: &str) -> Result<Value, String> {
    let out = run(kind);
    if !out.status.success() {
        return Err(format!(
            "status {:?}, stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let rows: Value = serde_json::from_slice(&out.stdout).unwrap();
    Ok(rows[0].clone())
}

#[test]
fn a_file_swapped_for_a_non_regular_entry_mid_query_reads_as_null() {
    let read: Vec<_> = kinds()
        .into_iter()
        .map(|kind| (kind, outcome(kind)))
        .collect();
    let expected: Vec<_> = kinds()
        .into_iter()
        .map(|kind| (kind, Ok(serde_json::json!({"done": kind, "content": null}))))
        .collect();
    assert_eq!(
        read, expected,
        "d/b.txt was swapped after the scan; the query must return with its content NULL \
         (a killed status = the read never returned)"
    );
}
