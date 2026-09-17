//! Watch/scan correctness: a live `mkdir` under the root must not insert a
//! directory row, renaming a matching file *out* of the tree must delete its
//! rows, a populated directory renamed *into* the tree must index every
//! file it brought, and one renamed *out* must delete every row it held. All
//! drive a **real** `notify` watcher over real temp directories through the
//! SDK public API — the core's integration tier.

use dirsql::{DirSQL, RowEvent, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

/// A table matching *every* path (like the default `files` table's `**/*`),
/// so a newly created subdirectory is a matcher candidate on the watch path.
fn files_table(root: &std::path::Path) -> Table {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    Table::new(
        "files",
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

/// Staging a directory elsewhere and renaming it into the root is the
/// standard atomic-publish pattern. inotify reports it as a single event
/// naming the directory — there are no per-child events — so the watch has to
/// walk the arriving subtree itself or its files are never indexed.
#[test]
fn dir_moved_into_root_indexes_its_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let stage = tempfile::TempDir::new().unwrap();
    fs::create_dir(stage.path().join("moved")).unwrap();
    fs::write(stage.path().join("moved/a.txt"), "alpha").unwrap();

    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::rename(stage.path().join("moved"), dir.path().join("moved")).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !paths(&db).iter().any(|p| p == "moved/a.txt") {
        db.poll_events(Duration::from_millis(200)).unwrap();
    }

    assert!(
        paths(&db).iter().any(|p| p == "moved/a.txt"),
        "a directory moved into the root must index the files it brought; rows: {:?}",
        paths(&db)
    );
    assert!(
        !paths(&db).iter().any(|p| p == "moved"),
        "the moved directory itself must not become a row; rows: {:?}",
        paths(&db)
    );
}

/// The same backfill has to reach every depth: one event names the top of the
/// arriving subtree and nothing below it.
#[test]
fn dir_moved_into_root_indexes_nested_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let stage = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(stage.path().join("moved/one/two")).unwrap();
    fs::write(stage.path().join("moved/top.txt"), "top").unwrap();
    fs::write(stage.path().join("moved/one/mid.txt"), "mid").unwrap();
    fs::write(stage.path().join("moved/one/two/deep.txt"), "deep").unwrap();

    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::rename(stage.path().join("moved"), dir.path().join("moved")).unwrap();

    let want = [
        "moved/top.txt",
        "moved/one/mid.txt",
        "moved/one/two/deep.txt",
    ];
    let has_all = |db: &DirSQL| {
        let rows = paths(db);
        want.iter().all(|w| rows.iter().any(|p| p == w))
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !has_all(&db) {
        db.poll_events(Duration::from_millis(200)).unwrap();
    }

    assert!(
        has_all(&db),
        "every file beneath a moved-in directory must be indexed; want {want:?}, rows: {:?}",
        paths(&db)
    );
}

/// The mirror of a move-in. A directory renamed out of the root arrives as
/// one event naming the directory and none for the files beneath it. The
/// directory is gone by then, so the rows are the only record of what it
/// held, and each must go with a Delete event.
#[test]
fn dir_moved_out_of_root_deletes_its_rows() {
    let dir = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("leaving/one")).unwrap();
    fs::write(dir.path().join("leaving/a.txt"), "alpha").unwrap();
    fs::write(dir.path().join("leaving/one/deep.txt"), "deep").unwrap();
    fs::write(dir.path().join("keep.txt"), "keep").unwrap();

    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    let under_leaving = |db: &DirSQL| -> Vec<String> {
        let mut rows: Vec<String> = paths(db)
            .into_iter()
            .filter(|p| p.starts_with("leaving/"))
            .collect();
        rows.sort();
        rows
    };
    assert_eq!(
        under_leaving(&db),
        vec![
            "leaving/a.txt".to_string(),
            "leaving/one/deep.txt".to_string()
        ],
        "initial scan should index both files under leaving/"
    );

    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::rename(dir.path().join("leaving"), outside.path().join("leaving")).unwrap();

    let mut deleted = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !under_leaving(&db).is_empty() {
        for e in db.poll_events(Duration::from_millis(200)).unwrap() {
            if let RowEvent::Delete { file_path, .. } = e {
                deleted.push(file_path);
            }
        }
    }

    assert!(
        under_leaving(&db).is_empty(),
        "a directory moved out of the root must delete the rows its files produced; rows: {:?}",
        paths(&db)
    );
    assert!(
        paths(&db).iter().any(|p| p == "keep.txt"),
        "a file outside the moved directory must keep its row; rows: {:?}",
        paths(&db)
    );
    deleted.sort();
    assert_eq!(
        deleted,
        vec![
            "leaving/a.txt".to_string(),
            "leaving/one/deep.txt".to_string()
        ],
        "one Delete event per file the directory held"
    );
}

/// A rename within the root is a move-out of the old name. Only the
/// disappearing side is asserted here; the new name is indexed by the
/// move-in walk (#1096).
#[test]
fn dir_renamed_within_root_deletes_rows_under_old_name() {
    let dir = tempfile::TempDir::new().unwrap();
    fs::create_dir(dir.path().join("a")).unwrap();
    fs::write(dir.path().join("a/x.txt"), "x").unwrap();

    let db = DirSQL::new(dir.path(), vec![files_table(dir.path())]).unwrap();
    assert!(
        paths(&db).iter().any(|p| p == "a/x.txt"),
        "initial scan should index a/x.txt; rows: {:?}",
        paths(&db)
    );

    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    fs::rename(dir.path().join("a"), dir.path().join("b")).unwrap();

    let mut saw_delete = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && paths(&db).iter().any(|p| p.starts_with("a/")) {
        for e in db.poll_events(Duration::from_millis(200)).unwrap() {
            if matches!(&e, RowEvent::Delete { file_path, .. } if file_path == "a/x.txt") {
                saw_delete = true;
            }
        }
    }

    assert!(
        !paths(&db).iter().any(|p| p.starts_with("a/")),
        "renaming a directory within the root must delete the rows under its old name; rows: {:?}",
        paths(&db)
    );
    assert!(
        saw_delete,
        "renaming a directory within the root must emit a Delete event for each file under the old name"
    );
}
