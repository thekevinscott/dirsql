//! Integration tests for the filesystem watcher: a **real** `notify` OS
//! watcher over a real temp directory (the unit-lint isolation rule keeps
//! this effectful tier out of `watcher.rs`'s inline unit module, which holds
//! the pure `translate_event` tests).

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{fs, thread};

use dirsql::watcher::{FileEvent, Watcher};
use dirsql::{DirSQL, RowEvent, Table, Value};
use tempfile::TempDir;

/// Drain events from `watcher` into a Vec, returning early once `done`
/// reports satisfaction or the `budget` elapses. Returns everything seen
/// so far. Used by the `detects_*` tests so the polling loop has no
/// data-dependent `break` body whose coverage region races with real-OS
/// filesystem-event timing.
fn collect_events_until(
    watcher: &Watcher,
    budget: Duration,
    done: impl Fn(&[FileEvent]) -> bool,
) -> Vec<FileEvent> {
    let deadline = Instant::now() + budget;
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        if let Some(event) = watcher.recv_timeout(Duration::from_millis(200)) {
            seen.push(event);
        }
    }
    seen
}

#[test]
fn detects_file_creation() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();

    // Small delay to let watcher initialize
    thread::sleep(Duration::from_millis(100));

    let file_path = dir.path().join("new_file.txt");
    fs::write(&file_path, "hello").unwrap();

    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(|e| matches!(e, FileEvent::Created(_)))
    });
    assert!(
        events.iter().any(|e| matches!(e, FileEvent::Created(_))),
        "Expected a Created event, saw: {events:?}"
    );
}

#[test]
fn detects_file_deletion() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("to_delete.txt");
    fs::write(&file_path, "doomed").unwrap();

    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    fs::remove_file(&file_path).unwrap();

    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(|e| matches!(e, FileEvent::Deleted(_)))
    });
    assert!(
        events.iter().any(|e| matches!(e, FileEvent::Deleted(_))),
        "Expected a Deleted event, saw: {events:?}"
    );
}

#[test]
fn detects_file_modification() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("modify_me.txt");
    fs::write(&file_path, "original").unwrap();

    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    fs::write(&file_path, "modified content").unwrap();

    // Some backends emit Create on overwrite, so accept Modified or Created.
    let matches_event = |e: &FileEvent| matches!(e, FileEvent::Modified(_) | FileEvent::Created(_));
    let events = collect_events_until(&watcher, Duration::from_secs(5), |seen| {
        seen.iter().any(matches_event)
    });
    assert!(
        events.iter().any(matches_event),
        "Expected a Modified or Created event, saw: {events:?}"
    );
}

#[test]
fn recv_blocks_until_event_arrives() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));
    fs::write(dir.path().join("recv_test.txt"), "hi").unwrap();
    let event = watcher.recv();
    assert!(event.is_some());
}

// ---------------------------------------------------------------------------
// Fan-out on the watch path: a create/delete for a file matching two tables'
// globs fires row events for BOTH tables and mutates BOTH (#580).
// ---------------------------------------------------------------------------

fn drain_row_events(
    db: &DirSQL,
    budget: Duration,
    done: impl Fn(&[RowEvent]) -> bool,
) -> Vec<RowEvent> {
    let deadline = Instant::now() + budget;
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    seen
}

fn tables_with_action<'a>(
    events: &'a [RowEvent],
    is_action: impl Fn(&RowEvent) -> Option<&str>,
) -> std::collections::HashSet<&'a str> {
    events.iter().filter_map(|e| is_action(e)).collect()
}

#[test]
fn watch_create_and_delete_fan_out_to_both_tables() {
    let dir = TempDir::new().unwrap();
    // Pre-create the target directory so notify's watch is installed before
    // the file is written (a new dir + immediate write races inotify).
    fs::create_dir_all(dir.path().join("data").join("2401.00001")).unwrap();

    let ta = Table::new("CREATE TABLE ta (col_a TEXT)", "data/*/metadata.json", |_| {
        vec![HashMap::from([("col_a".into(), Value::Text("A".into()))])]
    });
    let tb = Table::new(
        "CREATE TABLE tb (col_b TEXT)",
        "data/**/metadata.json",
        |_| vec![HashMap::from([("col_b".into(), Value::Text("B".into()))])],
    );

    let db = DirSQL::new(dir.path(), vec![ta, tb]).unwrap();
    db.start_watching().unwrap();
    thread::sleep(Duration::from_millis(250));

    let file = dir.path().join("data").join("2401.00001").join("metadata.json");
    fs::write(&file, "{}").unwrap();

    let created = drain_row_events(&db, Duration::from_secs(5), |seen| {
        let t = tables_with_action(seen, |e| match e {
            RowEvent::Insert { table, .. } => Some(table.as_str()),
            _ => None,
        });
        t.contains("ta") && t.contains("tb")
    });
    let inserted = tables_with_action(&created, |e| match e {
        RowEvent::Insert { table, .. } => Some(table.as_str()),
        _ => None,
    });
    assert!(
        inserted.contains("ta") && inserted.contains("tb"),
        "expected Insert events for both tables, saw: {created:?}"
    );
    assert_eq!(db.query("SELECT col_a FROM ta").unwrap().len(), 1);
    assert_eq!(db.query("SELECT col_b FROM tb").unwrap().len(), 1);

    fs::remove_file(&file).unwrap();

    let deleted = drain_row_events(&db, Duration::from_secs(5), |seen| {
        let t = tables_with_action(seen, |e| match e {
            RowEvent::Delete { table, .. } => Some(table.as_str()),
            _ => None,
        });
        t.contains("ta") && t.contains("tb")
    });
    let removed = tables_with_action(&deleted, |e| match e {
        RowEvent::Delete { table, .. } => Some(table.as_str()),
        _ => None,
    });
    assert!(
        removed.contains("ta") && removed.contains("tb"),
        "expected Delete events for both tables, saw: {deleted:?}"
    );
    assert_eq!(db.query("SELECT col_a FROM ta").unwrap().len(), 0);
    assert_eq!(db.query("SELECT col_b FROM tb").unwrap().len(), 0);
}

#[test]
fn try_recv_all_drains_pending_events() {
    let dir = TempDir::new().unwrap();
    let watcher = Watcher::new(dir.path()).unwrap();
    thread::sleep(Duration::from_millis(100));

    for i in 0..3 {
        fs::write(dir.path().join(format!("file_{i}.txt")), "data").unwrap();
    }

    thread::sleep(Duration::from_millis(500));

    let events = watcher.try_recv_all();
    assert!(
        !events.is_empty(),
        "Expected at least one event from batch file creation"
    );
}
