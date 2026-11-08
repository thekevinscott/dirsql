//! On Linux a watched file is read once its writer closes it, never while
//! it is still being written.
#![cfg(target_os = "linux")]

use dirsql::{DirSQL, RowEvent, Value};
use std::fs::{self, File};
use std::io::Write;
use std::time::{Duration, Instant};
use tempfile::TempDir;

const CONFIG: &str = r#"
[[table]]
name    = "items"
glob    = "*.json"
on-file = "cat"
ddl     = "CREATE TABLE items (name TEXT)"
"#;

fn names(db: &DirSQL) -> Vec<Value> {
    db.query("SELECT name FROM items")
        .unwrap()
        .into_iter()
        .map(|row| row["name"].clone())
        .collect()
}

#[test]
fn a_file_still_open_for_writing_is_not_read_until_it_closes() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let cfg = root.join(".dirsql.toml");
    fs::write(&cfg, CONFIG).unwrap();
    let db = DirSQL::builder().root(&root).config(&cfg).build().unwrap();
    db.set_fresh_reads(false);
    db.start_watching().unwrap();
    std::thread::sleep(Duration::from_millis(200));

    let mut file = File::create(root.join("a.json")).unwrap();
    file.write_all(br#"{"name": "#).unwrap();
    file.flush().unwrap();

    let mut while_open = Vec::new();
    let quiet_until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < quiet_until {
        while_open.extend(db.poll_events(Duration::from_millis(100)).unwrap());
    }
    assert!(while_open.is_empty(), "got: {while_open:?}");
    assert!(names(&db).is_empty());

    file.write_all(br#""alpha"}"#).unwrap();
    drop(file);

    let mut after_close = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && after_close.is_empty() {
        after_close.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    assert!(
        matches!(after_close.as_slice(), [RowEvent::Insert { .. }]),
        "got: {after_close:?}"
    );
    assert_eq!(names(&db), vec![Value::Text("alpha".into())]);
}
