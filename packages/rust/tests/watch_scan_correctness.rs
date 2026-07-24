//! Watch/scan correctness (#466): a live `mkdir` under the root must not
//! insert a directory row, and renaming a matching file *out* of the tree
//! must delete its rows. Both drive a **real** `notify` watcher over real temp
//! directories through the SDK public API — the core's integration tier.

use dirsql::{DirSQL, RowEvent, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

/// A table matching *every* path (like the default `files` table's `**/*`),
/// so a newly created subdirectory is a matcher candidate on the watch path.
fn files_table(root: &std::path::Path) -> Table {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    Table::new(
        "CREATE TABLE files (name TEXT, path TEXT)",
        "**/*",
        move |path| {
            let content = fs::read_to_string(path).unwrap_or_default();
            let abs = std::path::Path::new(path);
            let rel = abs
                .strip_prefix(&root)
                .unwrap_or(abs)
                .to_string_lossy()
                .into_owned();
            vec![HashMap::from([
                ("name".to_string(), Value::Text(content.trim().to_string())),
                ("path".to_string(), Value::Text(rel)),
            ])]
        },
    )
}

fn paths(db: &DirSQL) -> Vec<String> {
    db.query("SELECT path FROM files")
        .unwrap()
        .into_iter()
        .filter_map(|r| match r.get("path") {
            Some(Value::Text(p)) => Some(p.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn mkdir_under_root_inserts_no_row() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::create_dir(dir.path().join("subdir")).unwrap();

    let mut events = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        events.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }

    assert!(
        !paths(&db).iter().any(|p| p == "subdir"),
        "a mkdir'd directory must not become a row; rows: {:?}",
        paths(&db)
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            RowEvent::Insert { row, .. } if row.get("path") == Some(&Value::Text("subdir".into()))
        )),
        "a mkdir'd directory must not emit an Insert event; saw: {events:?}"
    );
}

#[test]
fn rename_out_deletes_rows() {
    let dir = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    fs::write(dir.path().join("a.txt"), "alpha").unwrap();

    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    assert!(
        paths(&db).iter().any(|p| p == "a.txt"),
        "initial scan should index a.txt; rows: {:?}",
        paths(&db)
    );

    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::rename(dir.path().join("a.txt"), outside.path().join("a.txt")).unwrap();

    let mut saw_delete = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && paths(&db).iter().any(|p| p == "a.txt") {
        for e in db.poll_events(Duration::from_millis(200)).unwrap() {
            if matches!(e, RowEvent::Delete { .. }) {
                saw_delete = true;
            }
        }
    }

    assert!(
        !paths(&db).iter().any(|p| p == "a.txt"),
        "renaming a file out of the tree must delete its rows; rows: {:?}",
        paths(&db)
    );
    assert!(
        saw_delete,
        "renaming a file out of the tree must emit a Delete event"
    );
}
