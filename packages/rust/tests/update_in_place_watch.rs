//! A watch refresh updates a keyed row in place, so its generated id holds.

use dirsql::{DirSQL, RowEvent, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const CONFIG: &str = r#"
[[table]]
name    = "experiments"
glob    = "*.json"
on-file = "cat"
ddl     = "CREATE TABLE experiments (id INTEGER PRIMARY KEY, folder_name TEXT UNIQUE, name TEXT)"
"#;

// Staged outside the glob and renamed into place: `fs::write` truncates
// before it writes, and the watcher can read the file in between.
fn write_experiment(root: &Path, folder: &str, name: &str) {
    let staged = root.join(format!("{folder}.tmp"));
    fs::write(
        &staged,
        format!("{{\"folder_name\":\"{folder}\",\"name\":\"{name}\"}}\n"),
    )
    .unwrap();
    fs::rename(&staged, root.join(format!("{folder}.json"))).unwrap();
}

fn id_of(db: &DirSQL, folder: &str) -> Value {
    let rows = db
        .query(&format!(
            "SELECT id FROM experiments WHERE folder_name = '{folder}'"
        ))
        .unwrap();
    assert_eq!(rows.len(), 1, "got: {rows:?}");
    rows[0]["id"].clone()
}

fn poll_until(db: &DirSQL, done: impl Fn(&[RowEvent]) -> bool) -> Vec<RowEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    seen
}

fn names_seen(events: &[RowEvent], name: &str) -> bool {
    events.iter().any(|event| match event {
        RowEvent::Insert { row, .. } => row["name"] == Value::Text(name.into()),
        RowEvent::Update { new_row, .. } => new_row["name"] == Value::Text(name.into()),
        _ => false,
    })
}

#[test]
fn renaming_an_experiment_keeps_its_id_and_emits_one_update() {
    let root = TempDir::new().unwrap();
    write_experiment(root.path(), "a", "Alpha");
    write_experiment(root.path(), "b", "Beta");
    let cfg = root.path().join(".dirsql.toml");
    fs::write(&cfg, CONFIG).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(&cfg)
        .build()
        .unwrap();
    let alpha = id_of(&db, "a");
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    write_experiment(root.path(), "0", "Zero");
    let added = poll_until(&db, |seen| names_seen(seen, "Zero"));
    assert!(
        matches!(added.as_slice(), [RowEvent::Insert { .. }]),
        "got: {added:?}"
    );

    write_experiment(root.path(), "a", "Renamed");
    let renamed = poll_until(&db, |seen| names_seen(seen, "Renamed"));
    match renamed.as_slice() {
        [
            RowEvent::Update {
                old_row, new_row, ..
            },
        ] => {
            assert_eq!(old_row["name"], Value::Text("Alpha".into()));
            assert_eq!(new_row["name"], Value::Text("Renamed".into()));
        }
        other => panic!("expected one update, got: {other:?}"),
    }
    assert_eq!(id_of(&db, "a"), alpha);
}
