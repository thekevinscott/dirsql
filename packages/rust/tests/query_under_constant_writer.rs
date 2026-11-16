//! A query that arrives while a batch of changes is being applied is answered
//! when that batch ends, before the next batch is applied, however many
//! changes the writer has queued behind it.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

const ROUNDS: usize = 25;

type Gate = Arc<Mutex<Option<(mpsc::Sender<i64>, mpsc::Receiver<()>)>>>;

/// Each file's name beside the number of the hook call that read it. Once
/// `gate` holds a sender, the next call reports its number and blocks until
/// the test answers.
fn gated_table(gate: Gate) -> Table {
    let calls = AtomicI64::new(0);
    Table::new(
        "item",
        "CREATE TABLE item (name TEXT, call INTEGER)",
        "items/*.txt",
        move |path| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            let armed = gate.lock().unwrap().take();
            if let Some((started, resume)) = armed {
                started.send(call).unwrap();
                resume.recv().unwrap();
            }
            let name = Path::new(path).file_stem().unwrap().to_string_lossy();
            vec![HashMap::from([
                ("name".to_string(), Value::Text(name.into_owned())),
                ("call".to_string(), Value::Integer(call)),
            ])] as Vec<Row>
        },
    )
}

fn integer(row: &Row, column: &str) -> i64 {
    match &row[column] {
        Value::Integer(n) => *n,
        other => panic!("{column} is {other:?}"),
    }
}

#[test]
fn a_query_returns_while_a_writer_never_pauses() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("items")).unwrap();
    let gate: Gate = Arc::new(Mutex::new(None));
    let db = DirSQL::new(&root, vec![gated_table(Arc::clone(&gate))]).unwrap();
    let write = |name: &str| fs::write(root.join("items").join(format!("{name}.txt")), "x");

    for round in 0..ROUNDS {
        let (started_tx, started_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        *gate.lock().unwrap() = Some((started_tx, resume_rx));
        write(&format!("in-progress-{round}")).unwrap();
        let in_progress = started_rx.recv().unwrap();

        // Written once the batch in progress has taken its events, so it can
        // only land in a later batch.
        write(&format!("next-{round}")).unwrap();
        let arrivals = db.index_arrivals();
        let sql = format!(
            "SELECT max(call) AS last, count(*) FILTER (WHERE name = 'next-{round}') AS next \
             FROM item"
        );
        let rows = thread::scope(|scope| {
            let query = scope.spawn(|| db.query(&sql));
            while db.index_arrivals() == arrivals {
                thread::yield_now();
            }
            // Gives the watcher time to queue the write, so the next batch is
            // ready to start the moment this one ends.
            thread::sleep(Duration::from_millis(20));
            resume_tx.send(()).unwrap();
            query.join().unwrap().unwrap()
        });

        assert!(
            integer(&rows[0], "last") >= in_progress,
            "round {round}: the query was answered before the batch in progress ended"
        );
        assert_eq!(
            integer(&rows[0], "next"),
            0,
            "round {round}: the next batch was applied before the query was answered"
        );
    }
}
