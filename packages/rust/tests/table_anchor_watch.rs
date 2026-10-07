//! The live watcher covers every table's anchor, not only the index root.

use dirsql::{DirSQL, Row, RowEvent, Table, Value};
use std::fs;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn name_table(name: &str, anchor: Option<&std::path::Path>) -> Table {
    let table = Table::new(
        name,
        format!("CREATE TABLE {name} (name TEXT)"),
        "**/*.txt",
        |path| {
            let body = fs::read_to_string(path).unwrap_or_default();
            vec![Row::from([(
                "name".to_string(),
                Value::Text(body.trim().to_string()),
            )])]
        },
    );
    match anchor {
        Some(anchor) => table.anchored(anchor),
        None => table,
    }
}

fn poll_until(db: &DirSQL, done: impl Fn(&[RowEvent]) -> bool) -> Vec<RowEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !done(&seen) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    seen
}

fn inserts_into(events: &[RowEvent], table: &str) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, RowEvent::Insert { table: t, .. } if t == table))
        .count()
}

#[test]
fn a_file_created_under_an_anchor_outside_the_root_emits_an_insert() {
    let root = TempDir::new().unwrap();
    let anchor = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(name_table("elsewhere", Some(anchor.path())))
        .build()
        .unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    fs::write(anchor.path().join("new.txt"), "hello").unwrap();

    let events = poll_until(&db, |seen| inserts_into(seen, "elsewhere") > 0);
    assert_eq!(inserts_into(&events, "elsewhere"), 1, "got: {events:?}");
    let rows = db.query("SELECT name FROM elsewhere").unwrap();
    assert_eq!(rows.len(), 1);
}

#[test]
fn the_root_and_an_outside_anchor_are_both_watched() {
    let root = TempDir::new().unwrap();
    let anchor = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .tables(vec![
            name_table("at_root", None),
            name_table("elsewhere", Some(anchor.path())),
        ])
        .build()
        .unwrap();
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    fs::write(root.path().join("a.txt"), "a").unwrap();
    fs::write(anchor.path().join("b.txt"), "b").unwrap();

    let events = poll_until(&db, |seen| {
        inserts_into(seen, "at_root") > 0 && inserts_into(seen, "elsewhere") > 0
    });
    assert_eq!(inserts_into(&events, "at_root"), 1, "got: {events:?}");
    assert_eq!(inserts_into(&events, "elsewhere"), 1, "got: {events:?}");
}
