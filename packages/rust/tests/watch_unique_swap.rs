//! A watch refresh may permute a UNIQUE value across a file's keyed rows:
//! only the end state must satisfy the constraint, as it does for a fresh
//! build of the same files.

use dirsql::{DirSQL, RowEvent, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const CONFIG: &str = r#"
[[table]]
name    = "t"
glob    = "d/*.jsonl"
on-file = "cat"
ddl     = "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT UNIQUE)"
"#;

// Staged outside the glob and renamed into place: `fs::write` truncates
// before it writes, and the watcher can read the file in between.
fn write_rows(root: &Path, body: &str) {
    let staged = root.join("d/a.tmp");
    fs::write(&staged, body).unwrap();
    fs::rename(&staged, root.join("d/a.jsonl")).unwrap();
}

fn rows(db: &DirSQL) -> Vec<(Value, Value)> {
    db.query("SELECT id, name FROM t ORDER BY id")
        .unwrap()
        .into_iter()
        .map(|row| (row["id"].clone(), row["name"].clone()))
        .collect()
}

fn named(id: i64, name: &str) -> (Value, Value) {
    (Value::Integer(id), Value::Text(name.into()))
}

fn poll_until(db: &DirSQL, done: impl Fn(&[RowEvent]) -> bool) -> Vec<RowEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    seen
}

#[test]
fn swapping_a_unique_value_between_keyed_rows_updates_both_in_place() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"x\"}\n{\"id\":2,\"name\":\"y\"}\n",
    );
    let cfg = root.path().join(".dirsql.toml");
    fs::write(&cfg, CONFIG).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(&cfg)
        .build()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    db.poll_events(Duration::ZERO).unwrap();

    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"y\"}\n{\"id\":2,\"name\":\"x\"}\n",
    );
    let events = poll_until(&db, |seen| seen.len() >= 2);

    let updates: Vec<(Value, Value)> = events
        .iter()
        .map(|event| match event {
            RowEvent::Update { new_row, .. } => (new_row["id"].clone(), new_row["name"].clone()),
            other => panic!("expected only updates, got: {other:?} in {events:?}"),
        })
        .collect();
    assert_eq!(updates.len(), 2, "events: {events:?}");
    assert!(updates.contains(&named(1, "y")), "events: {events:?}");
    assert!(updates.contains(&named(2, "x")), "events: {events:?}");
    assert_eq!(rows(&db), [named(1, "y"), named(2, "x")]);
}

#[test]
fn rotating_a_unique_value_through_three_keyed_rows_updates_each_in_place() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"x\"}\n{\"id\":2,\"name\":\"y\"}\n{\"id\":3,\"name\":\"z\"}\n",
    );
    let cfg = root.path().join(".dirsql.toml");
    fs::write(&cfg, CONFIG).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(&cfg)
        .build()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    db.poll_events(Duration::ZERO).unwrap();

    write_rows(
        root.path(),
        "{\"id\":1,\"name\":\"z\"}\n{\"id\":2,\"name\":\"x\"}\n{\"id\":3,\"name\":\"w\"}\n",
    );
    let events = poll_until(&db, |seen| seen.len() >= 3);

    assert_eq!(events.len(), 3, "events: {events:?}");
    assert!(
        events.iter().all(|e| matches!(e, RowEvent::Update { .. })),
        "events: {events:?}"
    );
    assert_eq!(rows(&db), [named(1, "z"), named(2, "x"), named(3, "w")]);
}
