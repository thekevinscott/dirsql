//! Once one query has had to wait for another, a cheap query is answered
//! while a slow one runs, and a watch batch lands mid-query without the slow
//! query reading any of it.

use dirsql::{DirSQL, RawFileEvent, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const ITEMS: i64 = 8;

/// Counts `item` while recursing on every row, so the scan of the table
/// spans the whole statement.
const SLOW: &str = "SELECT count(*) AS n FROM item WHERE (WITH RECURSIVE c(i) AS \
     (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < 2000000 + length(item.name) * 0) \
     SELECT count(*) FROM c) > 0";

const CHEAP: &str = "SELECT count(*) AS n FROM item";

const SETTLE: Duration = Duration::from_millis(300);

fn item_table() -> Table {
    Table::new(
        "item",
        "CREATE TABLE item (name TEXT)",
        "*.txt",
        |path: &str| {
            let name = Path::new(path).file_stem().unwrap().to_string_lossy();
            vec![HashMap::from([(
                "name".to_string(),
                Value::Text(name.into_owned()),
            )])] as Vec<Row>
        },
    )
}

fn count(rows: &[Row]) -> i64 {
    match rows[0]["n"] {
        Value::Integer(n) => n,
        ref other => panic!("n is {other:?}"),
    }
}

fn fixture() -> (TempDir, Arc<DirSQL>) {
    let dir = TempDir::new().unwrap();
    for i in 0..ITEMS {
        fs::write(dir.path().join(format!("{i}.txt")), "x").unwrap();
    }
    let db = Arc::new(DirSQL::new(dir.path(), vec![item_table()]).unwrap());
    (dir, db)
}

/// Runs [`SLOW`] on another thread; the flag is set once it returns.
fn slow_query(db: &Arc<DirSQL>) -> (thread::JoinHandle<i64>, Arc<AtomicBool>) {
    let done = Arc::new(AtomicBool::new(false));
    let handle = {
        let db = Arc::clone(db);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            let n = count(&db.query(SLOW).unwrap());
            done.store(true, Ordering::SeqCst);
            n
        })
    };
    thread::sleep(SETTLE);
    (handle, done)
}

/// The first overlap: a cheap query waits for a slow one, as it always has.
fn overlap_once(db: &Arc<DirSQL>) {
    let (slow, _) = slow_query(db);
    assert_eq!(count(&db.query(CHEAP).unwrap()), ITEMS);
    assert_eq!(slow.join().unwrap(), ITEMS);
}

#[test]
fn after_one_overlap_a_cheap_query_returns_while_a_slow_query_runs() {
    let (_dir, db) = fixture();
    overlap_once(&db);

    let (slow, done) = slow_query(&db);
    assert_eq!(count(&db.query(CHEAP).unwrap()), ITEMS);
    assert!(
        !done.load(Ordering::SeqCst),
        "a cheap query must be answered while a slow query runs once one overlap has been waited out"
    );
    assert_eq!(slow.join().unwrap(), ITEMS);
}

#[test]
fn a_batch_lands_while_a_slow_query_runs_and_the_slow_query_reads_none_of_it() {
    let (dir, db) = fixture();
    overlap_once(&db);

    let (slow, done) = slow_query(&db);
    let added = dir.path().join("added.txt");
    fs::write(&added, "x").unwrap();
    db.apply_file_events(vec![RawFileEvent::Created(added)]);
    assert!(
        !done.load(Ordering::SeqCst),
        "a watch batch must land while a slow query runs once one overlap has been waited out"
    );
    assert_eq!(count(&db.query(CHEAP).unwrap()), ITEMS + 1);
    assert_eq!(
        slow.join().unwrap(),
        ITEMS,
        "the slow query read a batch that landed mid-query"
    );
}
