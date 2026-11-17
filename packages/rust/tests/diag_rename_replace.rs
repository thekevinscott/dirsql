//! TEMPORARY diagnostic, removed before this branch is ready: prints what
//! Windows reports when a file is replaced by a rename, raw and through
//! dirsql.

#![cfg(windows)]

use dirsql::{DirSQL, RowEvent};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const ROUNDS: usize = 150;

fn write_rows(root: &Path, body: &str) {
    let staged = root.join("d/a.tmp");
    fs::write(&staged, body).unwrap();
    fs::rename(&staged, root.join("d/a.jsonl")).unwrap();
}

fn body(round: usize) -> &'static str {
    if round % 2 == 0 {
        "{\"id\":1,\"name\":\"y\"}\n{\"id\":2,\"name\":\"x\"}\n"
    } else {
        "{\"id\":1,\"name\":\"x\"}\n{\"id\":2,\"name\":\"y\"}\n"
    }
}

fn raw_sequences() -> BTreeMap<String, usize> {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    write_rows(root.path(), body(1));
    let (tx, rx) = mpsc::channel::<Event>();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        notify::Config::default(),
    )
    .unwrap();
    watcher
        .watch(root.path(), RecursiveMode::Recursive)
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut tally = BTreeMap::new();
    for round in 0..ROUNDS {
        write_rows(root.path(), body(round));
        let mut seen = Vec::new();
        let mut last = Instant::now();
        let mut widest = Duration::ZERO;
        while let Ok(event) = rx.recv_timeout(Duration::from_millis(150)) {
            if !seen.is_empty() {
                widest = widest.max(last.elapsed());
            }
            last = Instant::now();
            let names: Vec<String> = event
                .paths
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            seen.push(format!("{:?} {}", event.kind, names.join(",")));
        }
        let gap = if widest > Duration::from_millis(1) {
            " [gap >1ms]"
        } else {
            ""
        };
        *tally
            .entry(format!("{}{gap}", seen.join(" | ")))
            .or_insert(0) += 1;
    }
    tally
}

fn row_event_sequences() -> BTreeMap<String, usize> {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    write_rows(root.path(), body(1));
    let cfg = root.path().join(".dirsql.toml");
    fs::write(
        &cfg,
        "[[table]]\nname = \"t\"\nglob = \"d/*.jsonl\"\non-file = \"cat\"\nddl = \"CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT UNIQUE)\"\n",
    )
    .unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(&cfg)
        .build()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    db.poll_events(Duration::ZERO).unwrap();
    let mut tally = BTreeMap::new();
    for round in 0..ROUNDS {
        write_rows(root.path(), body(round));
        let mut seen = Vec::new();
        let mut batches = 0;
        loop {
            let events = db.poll_events(Duration::from_millis(400)).unwrap();
            if events.is_empty() {
                break;
            }
            batches += 1;
            for event in events {
                seen.push(match event {
                    RowEvent::Insert { .. } => "insert".to_string(),
                    RowEvent::Update { .. } => "update".to_string(),
                    RowEvent::Delete { .. } => "delete".to_string(),
                    RowEvent::Error { error, .. } => format!("error({error})"),
                });
            }
        }
        *tally
            .entry(format!("{} in {batches} poll(s)", seen.join(",")))
            .or_insert(0) += 1;
    }
    tally
}

#[test]
fn what_windows_reports_for_a_file_replaced_by_a_rename() {
    let raw = raw_sequences();
    let rows = row_event_sequences();
    panic!("DIAGNOSTIC\nraw notify sequences: {raw:#?}\nrow event sequences: {rows:#?}");
}
