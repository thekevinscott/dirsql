//! On Linux a folder created while the instance runs, gathering children that
//! name parents written before them in another folder, never fails a query,
//! however slowly the changes apply.

#![cfg(target_os = "linux")]

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// How long the first advice takes to apply, while the writer runs ahead.
const SLOW: Duration = Duration::from_millis(200);

fn turns(root: &Path) -> PathBuf {
    root.join("windows/0/forks/0/turns")
}

fn dir_name(path: &str) -> String {
    Path::new(path)
        .parent()
        .and_then(Path::file_name)
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// `advices/<id>/advice.txt`: one `advice` row, keyed by its directory.
fn advice_table() -> Table {
    Table::new(
        "advice",
        "CREATE TABLE advice (id TEXT PRIMARY KEY)",
        "advices/*/advice.txt",
        |path| {
            let id = dir_name(path);
            if id == format!("{:064x}", 0) {
                thread::sleep(SLOW);
            }
            vec![HashMap::from([("id".into(), Value::Text(id))])] as Vec<Row>
        },
    )
}

/// `windows/0/forks/0/turns/<n>/metadata.txt`: one `turn` row naming the
/// advice it references. A file not yet written fails to parse.
fn turn_table() -> Table {
    Table::try_new(
        "turn",
        "CREATE TABLE turn (n TEXT, advice TEXT NOT NULL REFERENCES advice(id))",
        "windows/*/forks/*/turns/*/metadata.txt",
        |path| {
            let advice = fs::read_to_string(path)?;
            if advice.is_empty() {
                return Err("no advice yet".into());
            }
            Ok(vec![HashMap::from([
                ("n".into(), Value::Text(dir_name(path))),
                ("advice".into(), Value::Text(advice)),
            ])])
        },
    )
}

/// The advice, then the turn that references it; the first turn creates the
/// folder that gathers them.
fn write_pair(root: &Path, n: usize) {
    let advice = format!("{n:064x}");
    fs::create_dir(root.join("advices").join(&advice)).unwrap();
    fs::write(root.join("advices").join(&advice).join("advice.txt"), "x").unwrap();
    let turn = turns(root).join(n.to_string());
    fs::create_dir_all(&turn).unwrap();
    fs::write(turn.join("metadata.txt"), &advice).unwrap();
}

const PAIRS: &str = "SELECT count(*) AS n FROM turn JOIN advice ON advice.id = turn.advice";

fn count(db: &DirSQL) -> i64 {
    match db.query(PAIRS).unwrap()[0].get("n") {
        Some(Value::Integer(n)) => *n,
        other => panic!("expected a count, got {other:?}"),
    }
}

#[test]
fn queries_never_fail_while_a_new_folder_gathers_children_of_another_folder() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir_all(root.join("windows/0/forks/0")).unwrap();
    fs::create_dir(root.join("advices")).unwrap();
    let db = DirSQL::new(&root, vec![advice_table(), turn_table()]).unwrap();
    assert!(!turns(&root).exists());

    let stop = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicUsize::new(0));
    let writer = {
        let (root, stop, written) = (root.clone(), Arc::clone(&stop), Arc::clone(&written));
        thread::spawn(move || {
            let mut n = 0;
            while !stop.load(Ordering::SeqCst) {
                write_pair(&root, n);
                n += 1;
                written.store(n, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let mut failures = Vec::new();
    let mut queries = 0;
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        queries += 1;
        if let Err(e) = db.query(PAIRS) {
            failures.push(e.to_string());
        }
    }
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(
        failures.is_empty(),
        "{} of {queries} queries failed; first: {}",
        failures.len(),
        failures[0]
    );

    let pairs = i64::try_from(written.load(Ordering::SeqCst)).unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    while count(&db) != pairs && Instant::now() < end {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(count(&db), pairs);
}
