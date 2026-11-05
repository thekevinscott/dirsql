//! The live index tracks files reached through a symlinked directory as a
//! fresh scan of the tree would: a change at the link's target is a change
//! at the link. Events are read without a query in between, since a query
//! reconciles the index with the disk on its own.

use dirsql::{DirSQL, Row, RowEvent, Table, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

/// `real/x.txt` holding `body`, and `link -> real`.
fn linked_tree(body: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/x.txt"), body).unwrap();
    symlink_dir(&root.path().join("real"), &root.path().join("link"));
    root
}

fn body_table() -> Table {
    Table::new("t", "CREATE TABLE t (body TEXT)", "*/*.txt", |path| {
        let body = fs::read_to_string(path).unwrap_or_default();
        vec![Row::from([(
            "body".to_string(),
            Value::Text(body.trim().to_string()),
        )])]
    })
}

fn watching(root: &Path, table: Table) -> DirSQL {
    let db = DirSQL::builder().root(root).table(table).build().unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    db
}

fn poll_until(db: &DirSQL, done: impl Fn(&[RowEvent]) -> bool) -> Vec<RowEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    seen
}

fn paths_of(events: &[RowEvent], kind: fn(&RowEvent) -> bool) -> Vec<String> {
    let mut paths: Vec<String> = events
        .iter()
        .filter(|event| kind(event))
        .map(|event| match event {
            RowEvent::Insert { file_path, .. }
            | RowEvent::Update { file_path, .. }
            | RowEvent::Delete { file_path, .. } => file_path.clone(),
            RowEvent::Error { file_path, .. } => file_path.to_string_lossy().into_owned(),
        })
        .collect();
    paths.sort();
    paths
}

fn is_insert(event: &RowEvent) -> bool {
    matches!(event, RowEvent::Insert { .. })
}

fn is_update(event: &RowEvent) -> bool {
    matches!(event, RowEvent::Update { .. })
}

fn is_delete(event: &RowEvent) -> bool {
    matches!(event, RowEvent::Delete { .. })
}

fn is_error(event: &RowEvent) -> bool {
    matches!(event, RowEvent::Error { .. })
}

#[test]
fn a_target_moved_out_of_the_tree_deletes_the_rows_reached_through_the_link() {
    let root = linked_tree("one");
    let away = TempDir::new().unwrap();
    let db = watching(root.path(), body_table());

    fs::rename(root.path().join("real"), away.path().join("real")).unwrap();

    let events = poll_until(&db, |seen| paths_of(seen, is_delete).len() >= 2);
    assert_eq!(
        paths_of(&events, is_delete),
        ["link/x.txt", "real/x.txt"],
        "got: {events:?}"
    );
}

#[test]
fn a_target_moved_back_restores_the_rows_reached_through_the_link() {
    let root = linked_tree("one");
    let away = TempDir::new().unwrap();
    let db = watching(root.path(), body_table());
    fs::rename(root.path().join("real"), away.path().join("real")).unwrap();
    poll_until(&db, |seen| paths_of(seen, is_delete).len() >= 2);

    fs::rename(away.path().join("real"), root.path().join("real")).unwrap();

    let events = poll_until(&db, |seen| paths_of(seen, is_insert).len() >= 2);
    assert_eq!(
        paths_of(&events, is_insert),
        ["link/x.txt", "real/x.txt"],
        "got: {events:?}"
    );
}

#[test]
fn an_edit_at_the_target_updates_the_rows_reached_through_the_link() {
    let root = linked_tree("one");
    let db = watching(root.path(), body_table());

    fs::write(root.path().join("real/x.txt"), "two").unwrap();

    let events = poll_until(&db, |seen| paths_of(seen, is_update).len() >= 2);
    assert_eq!(
        paths_of(&events, is_update),
        ["link/x.txt", "real/x.txt"],
        "got: {events:?}"
    );
}

#[test]
fn a_per_table_hook_is_not_handed_a_path_whose_target_left_the_tree() {
    let root = linked_tree("{\"body\":\"one\"}\n");
    fs::write(
        root.path().join(".dirsql.toml"),
        "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (body TEXT)\"\nglob = \"*/*.txt\"\non-file = \"cat\"\n",
    )
    .unwrap();
    let away = TempDir::new().unwrap();
    let db = DirSQL::from_config_path(root.path().join(".dirsql.toml")).unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    fs::rename(root.path().join("real"), away.path().join("real")).unwrap();

    let events = poll_until(&db, |seen| {
        !paths_of(seen, is_error).is_empty() || paths_of(seen, is_delete).len() >= 2
    });
    assert!(paths_of(&events, is_error).is_empty(), "got: {events:?}");
    assert_eq!(paths_of(&events, is_delete).len(), 2, "got: {events:?}");
}
