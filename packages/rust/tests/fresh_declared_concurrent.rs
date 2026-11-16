//! Every write reaches the index while another thread consumes the
//! instance's events, and a change applied before `watch()` starts reaches no
//! stream.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use dirsql::{DirSQL, RowEvent};
use fresh::{Tree, await_lines, text};
use futures_executor::block_on;
use futures_util::StreamExt;
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
        await_lines(db, &rel, &[format!("c{i}")]);
        tree.write(&rel, &format!("e{i}"));
        await_lines(db, &rel, &[format!("e{i}")]);
        tree.remove(&rel);
        await_lines::<&str>(db, &rel, &[]);
    }
}

#[test]
fn every_write_reaches_the_index_while_another_thread_consumes_watch() {
    let tree = Tree::new();
    let db = tree.open();
    let mut stream = db.watch().unwrap();
    thread::spawn(move || block_on(async { while stream.next().await.is_some() {} }));
    create_edit_delete(&tree, &db);
}

#[test]
fn every_write_reaches_the_index_while_another_thread_polls_events() {
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
fn a_change_applied_before_watch_starts_reaches_no_stream() {
    let tree = Tree::new();
    let db = tree.open();
    for i in 0..FILES {
        let rel = format!("g/{i}.txt");
        tree.write(&rel, &format!("g{i}"));
        await_lines(&db, &rel, &[format!("g{i}")]);
    }
    let mut stream = db.watch().unwrap();
    tree.write("sentinel.txt", "end");
    match block_on(stream.next()).unwrap() {
        RowEvent::Insert { table, row, .. } if table == "notes" => {
            assert_eq!(text(&row["line"]), "end");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
