//! On Linux a writer creating a parent and then its child, in new
//! directories, never fails a query, and every write reaches the index.

#![cfg(target_os = "linux")]

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

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
        |path| vec![HashMap::from([("id".into(), Value::Text(dir_name(path)))])] as Vec<Row>,
    )
}

/// `turns/<n>/metadata.txt`: one `turn` row naming the advice it references.
/// A file not yet written holds no reference and fails to parse.
fn turn_table() -> Table {
    Table::try_new(
        "turn",
        "CREATE TABLE turn (n TEXT, advice TEXT REFERENCES advice(id))",
        "turns/*/metadata.txt",
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

fn open() -> (TempDir, PathBuf, DirSQL) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("advices")).unwrap();
    fs::create_dir(root.join("turns")).unwrap();
    let db = DirSQL::new(&root, vec![advice_table(), turn_table()]).unwrap();
    (dir, root, db)
}

/// The advice, then the turn that references it, each in a new directory.
fn write_pair(root: &Path, n: usize) {
    let advice = format!("{n:064x}");
    fs::create_dir(root.join("advices").join(&advice)).unwrap();
    fs::write(root.join("advices").join(&advice).join("advice.txt"), "x").unwrap();
    fs::create_dir(root.join("turns").join(n.to_string())).unwrap();
    fs::write(
        root.join("turns").join(n.to_string()).join("metadata.txt"),
        &advice,
    )
    .unwrap();
}

fn count(db: &DirSQL, sql: &str) -> i64 {
    match db.query(sql).unwrap()[0].get("n") {
        Some(Value::Integer(n)) => *n,
        other => panic!("expected a count, got {other:?}"),
    }
}

#[test]
fn queries_never_fail_while_a_writer_creates_parents_then_children() {
    let (_dir, root, db) = open();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let root = root.clone();
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut n = 0;
            while !stop.load(Ordering::SeqCst) {
                write_pair(&root, n);
                n += 1;
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let mut failures = Vec::new();
    let mut queries = 0;
    let end = Instant::now() + Duration::from_secs(3);
    while Instant::now() < end {
        queries += 1;
        if let Err(e) =
            db.query("SELECT count(*) AS n FROM turn JOIN advice ON advice.id = turn.advice")
        {
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
}

fn await_count(db: &DirSQL, sql: &str, expected: i64) {
    let end = Instant::now() + Duration::from_secs(10);
    while count(db, sql) != expected && Instant::now() < end {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(count(db, sql), expected, "{sql}");
}

#[test]
fn every_write_reaches_the_index() {
    let (_dir, root, db) = open();
    for n in 0..200 {
        write_pair(&root, n);
    }
    await_count(&db, "SELECT count(*) AS n FROM advice", 200);
    await_count(&db, "SELECT count(*) AS n FROM turn", 200);
}
