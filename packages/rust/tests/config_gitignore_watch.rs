//! A live config table skips a gitignored file the way its startup scan does.
//! Real `notify` watcher, real filesystem, real SQLite, SDK public API.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{fs, thread};

use dirsql::{DirSQL, RowEvent, Table, Value};
use tempfile::TempDir;

fn names(db: &DirSQL) -> Vec<String> {
    let mut out: Vec<String> = db
        .query("SELECT name FROM t")
        .unwrap()
        .iter()
        .map(|r| match r.get("name") {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("name was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_gitignored_file_created_while_watching_is_not_indexed() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".gitignore"), "skip.md\n").unwrap();
    let table = Table::new("t", "CREATE TABLE t (name TEXT)", "*.md", |path| {
        let name = path.rsplit(['/', '\\']).next().unwrap().to_string();
        vec![HashMap::from([("name".into(), Value::Text(name))])]
    });
    let db = DirSQL::new(dir.path(), vec![table]).unwrap();
    db.start_watching().unwrap();
    thread::sleep(Duration::from_millis(250));

    fs::write(dir.path().join("skip.md"), "x").unwrap();
    fs::write(dir.path().join("keep.md"), "x").unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    while Instant::now() < deadline && !seen.iter().any(|e| matches!(e, RowEvent::Insert { .. })) {
        seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());
    }
    thread::sleep(Duration::from_millis(500));
    seen.extend(db.poll_events(Duration::from_millis(200)).unwrap());

    assert_eq!(names(&db), vec!["keep.md"], "events: {seen:?}");
}
