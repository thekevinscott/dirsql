//! A query reads the index; it does not wait for the watcher's events. Under a
//! writer that never pauses, whose events take longer to apply than to produce,
//! a query still returns.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const FILES: usize = 100;

fn slow_table() -> Table {
    Table::new(
        "item",
        "CREATE TABLE item (path TEXT)",
        "items/*.txt",
        |path| {
            thread::sleep(Duration::from_millis(2));
            vec![HashMap::from([(
                "path".into(),
                Value::Text(path.to_string()),
            )])] as Vec<Row>
        },
    )
}

#[test]
fn a_query_returns_while_a_writer_never_pauses() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("items")).unwrap();
    let db = DirSQL::new(&root, vec![slow_table()]).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let root = root.clone();
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut n: usize = 0;
            while !stop.load(Ordering::SeqCst) {
                fs::write(
                    root.join("items").join(format!("{}.txt", n % FILES)),
                    n.to_string(),
                )
                .unwrap();
                n += 1;
            }
        })
    };
    thread::sleep(Duration::from_millis(200));

    let (tx, rx) = mpsc::channel();
    {
        let db = db.clone();
        thread::spawn(move || {
            let _ = tx.send(db.query("SELECT count(*) AS n FROM item").map(|_| ()));
        });
    }
    let answered = rx.recv_timeout(Duration::from_secs(5));
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap();
    assert!(
        matches!(answered, Ok(Ok(()))),
        "the query did not return within 5s under a constant writer: {answered:?}"
    );
}
