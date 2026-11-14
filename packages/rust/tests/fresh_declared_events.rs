//! Each change reaches `watch()` and `poll_events()` consumers exactly once.

#![cfg(target_os = "linux")]

#[path = "common/fresh.rs"]
mod fresh;

use dirsql::{DirSQL, RowEvent};
use fresh::{Tree, await_lines, text};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const FILES: usize = 200;

/// How many inserts each `notes` line got, after events stop arriving.
fn tally(mut next: impl FnMut() -> Vec<RowEvent>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut quiet_since = Instant::now();
    while Instant::now() < deadline {
        let batch = next();
        if batch.is_empty() {
            if counts.len() >= FILES && quiet_since.elapsed() > Duration::from_secs(1) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        quiet_since = Instant::now();
        for event in batch {
            match event {
                RowEvent::Insert { table, row, .. } if table == "notes" => {
                    *counts.entry(text(&row["line"])).or_insert(0) += 1;
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
    }
    counts
}

fn expected() -> BTreeMap<String, usize> {
    (0..FILES).map(|i| (format!("w{i}"), 1)).collect()
}

fn write_and_read_each(tree: &Tree, db: &DirSQL) {
    for i in 0..FILES {
        let rel = format!("w/{i}.txt");
        tree.write(&rel, &format!("w{i}"));
        await_lines(db, &rel, &[format!("w{i}")]);
    }
}

#[test]
fn a_watch_stream_gets_each_change_once() {
    let tree = Tree::new();
    let db = tree.open();
    let mut stream = db.watch().unwrap();
    write_and_read_each(&tree, &db);
    let counts = tally(|| {
        let mut batch = Vec::new();
        while let Ok(event) = stream.try_recv() {
            batch.push(event);
        }
        batch
    });
    assert_eq!(counts, expected());
}

#[test]
fn poll_events_returns_each_change_once() {
    let tree = Tree::new();
    let db = tree.open();
    assert!(db.poll_events(Duration::ZERO).unwrap().is_empty());
    write_and_read_each(&tree, &db);
    let counts = tally(|| db.poll_events(Duration::from_millis(50)).unwrap());
    assert_eq!(counts, expected());
}
