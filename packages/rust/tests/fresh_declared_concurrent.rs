//! A query issued the moment a write returns reads that write while another
//! thread consumes the instance's events, and a change read before `watch()`
//! starts still reaches the stream, once.

#[path = "common/fresh.rs"]
mod fresh;

use dirsql::{DirSQL, RowEvent};
use fresh::{Tree, lines_of, text};
use futures_executor::block_on;
use futures_util::StreamExt;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const ROUNDS: usize = 1000;
const FILES: usize = 200;

fn create_edit_delete(tree: &Tree, db: &DirSQL) {
    for i in 0..ROUNDS {
        let rel = format!("h/{i}.txt");
        tree.write(&rel, &format!("c{i}"));
        assert_eq!(lines_of(db, &rel), [format!("c{i}")], "create {i}");
        tree.write(&rel, &format!("e{i}"));
        assert_eq!(lines_of(db, &rel), [format!("e{i}")], "edit {i}");
        tree.remove(&rel);
        assert!(lines_of(db, &rel).is_empty(), "delete {i}");
    }
}

#[test]
fn every_write_is_read_by_the_next_query_while_another_thread_consumes_watch() {
    let tree = Tree::new();
    let db = tree.open();
    let mut stream = db.watch().unwrap();
    thread::spawn(move || block_on(async { while stream.next().await.is_some() {} }));
    create_edit_delete(&tree, &db);
}

#[test]
fn every_write_is_read_by_the_next_query_while_another_thread_polls_events() {
    let tree = Tree::new();
    let db = tree.open();
    let stop = Arc::new(AtomicBool::new(false));
    let poller = {
        let db = db.clone();
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                db.poll_events(Duration::from_millis(5)).unwrap();
            }
        })
    };
    create_edit_delete(&tree, &db);
    stop.store(true, Ordering::SeqCst);
    poller.join().unwrap();
}

#[test]
fn a_change_read_before_watch_starts_reaches_the_stream_once() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..FILES {
        let rel = format!("g/{i}.txt");
        tree.write(&rel, &format!("g{i}"));
        assert_eq!(lines_of(&db, &rel), [format!("g{i}")], "file {i}");
    }
    let mut stream = db.watch().unwrap();
    // Unread by any query, so only the watcher reports it, after every
    // change before it.
    tree.write("sentinel.txt", "end");
    let mut counts = BTreeMap::new();
    loop {
        match block_on(stream.next()).unwrap() {
            RowEvent::Insert { table, row, .. } if table == "notes" => {
                let line = text(&row["line"]);
                if line == "end" {
                    break;
                }
                *counts.entry(line).or_insert(0) += 1;
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
    let expected: BTreeMap<String, usize> = (0..FILES).map(|i| (format!("g{i}"), 1)).collect();
    assert_eq!(counts, expected);
}
