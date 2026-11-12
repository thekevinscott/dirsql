//! While `lock_index` holds the index, queries, `apply_queued_events` and watch
//! batches wait for `unlock_index`; taking the lock waits for a read already
//! in progress and for another holder.

#[path = "common/fresh.rs"]
mod fresh;

use dirsql::{DirSQL, Table, Value};
use fresh::Tree;
#[cfg(target_os = "linux")]
use fresh::lines_of;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Runs `work` on another thread and reports whether it was still running
/// when `release` ran, then waits for it.
fn waits_for<T: Send>(work: impl FnOnce() -> T + Send, release: impl FnOnce()) -> (bool, T) {
    thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let handle = scope.spawn(move || {
            let out = work();
            tx.send(()).unwrap();
            out
        });
        let early = rx.recv_timeout(Duration::from_millis(200)).is_ok();
        release();
        (!early, handle.join().unwrap())
    })
}

#[test]
#[cfg(target_os = "linux")]
fn a_query_waits_for_unlock_and_then_reads_the_disk() {
    let tree = Tree::new();
    tree.write("a.txt", "one");
    let db = tree.open();
    db.lock_index().unwrap();
    let (waited, lines) = waits_for(
        || lines_of(&db, "b.txt"),
        || {
            tree.write("b.txt", "two");
            db.unlock_index().unwrap();
        },
    );
    assert!(waited);
    assert_eq!(lines, ["two"]);
}

#[test]
fn apply_queued_events_waits_for_unlock() {
    let tree = Tree::new();
    let db = tree.open();
    db.lock_index().unwrap();
    let (waited, read) = waits_for(|| db.apply_queued_events(), || db.unlock_index().unwrap());
    assert!(waited);
    read.unwrap();
}

#[test]
fn a_watch_batch_waits_for_unlock() {
    let tree = Tree::new();
    let db = tree.open();
    db.lock_index().unwrap();
    let (waited, _) = waits_for(
        || db.apply_file_events(Vec::new()),
        || db.unlock_index().unwrap(),
    );
    assert!(waited);
}

#[test]
fn a_second_lock_waits_for_the_first_holder() {
    let tree = Tree::new();
    let db = tree.open();
    db.lock_index().unwrap();
    let (waited, locked) = waits_for(|| db.lock_index(), || db.unlock_index().unwrap());
    assert!(waited);
    locked.unwrap();
    db.unlock_index().unwrap();
}

type Gate = Arc<Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>>;

/// A table whose hook, once `gate` holds a sender, reports that it started
/// and blocks until the test answers.
fn gated_table(gate: Gate) -> Table {
    Table::new(
        "items",
        "CREATE TABLE items (name TEXT)",
        "*.txt",
        move |_path| {
            if let Some((started, resume)) = gate.lock().unwrap().take() {
                started.send(()).unwrap();
                resume.recv().unwrap();
            }
            vec![HashMap::from([(
                "name".to_string(),
                Value::Text("x".to_string()),
            )])]
        },
    )
}

#[test]
fn taking_the_lock_waits_for_a_read_in_progress() {
    let tree = Tree::new();
    let gate = Arc::new(Mutex::new(None));
    let db = DirSQL::new(tree.root(), vec![gated_table(Arc::clone(&gate))]).unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    *gate.lock().unwrap() = Some((started_tx, resume_rx));
    tree.write("a.txt", "one");
    thread::scope(|scope| {
        let reader = scope.spawn(|| db.query("SELECT name FROM items").unwrap());
        started_rx.recv().unwrap();
        let (waited, locked) = waits_for(|| db.lock_index(), || resume_tx.send(()).unwrap());
        assert!(waited);
        locked.unwrap();
        assert_eq!(reader.join().unwrap().len(), 1);
    });
    db.unlock_index().unwrap();
}
