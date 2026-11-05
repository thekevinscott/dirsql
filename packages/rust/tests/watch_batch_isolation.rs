//! A query never sees a watch batch half applied, nor one its foreign keys
//! roll back: it waits for the batch to end. Queries here skip reading the
//! disk themselves, as the TypeScript binding's do, so only the batch writes.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use dirsql::watcher::FileEvent;
use dirsql::{DirSQL, Row, Table, Value};
use tempfile::TempDir;

const HOLD: Duration = Duration::from_secs(1);

/// The hooks of one batch: the second hook to read `target` reports that the
/// batch is under way, then holds it open for [`HOLD`].
#[derive(Clone)]
struct Pacer {
    target: &'static str,
    calls: Arc<AtomicUsize>,
    started: Arc<Mutex<Sender<()>>>,
}

impl Pacer {
    fn new(target: &'static str) -> (Self, Receiver<()>) {
        let (tx, rx) = channel();
        let pacer = Self {
            target,
            calls: Arc::default(),
            started: Arc::new(Mutex::new(tx)),
        };
        (pacer, rx)
    }

    fn table(&self, name: &str, ddl: &str, glob: &str, row: fn(&str) -> Row) -> Table {
        let pacer = self.clone();
        Table::new(name, ddl, glob, move |path: &str| {
            if Path::new(path).ends_with(pacer.target)
                && pacer.calls.fetch_add(1, Ordering::SeqCst) == 1
            {
                pacer.started.lock().unwrap().send(()).unwrap();
                thread::sleep(HOLD);
            }
            vec![row(path)]
        })
    }
}

fn line_row(path: &str) -> Row {
    HashMap::from([(
        "line".into(),
        Value::Text(fs::read_to_string(path).unwrap().trim().to_string()),
    )])
}

fn child_row(path: &str) -> Row {
    HashMap::from([
        (
            "id".into(),
            Value::Text(
                Path::new(path)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into(),
            ),
        ),
        (
            "parent".into(),
            Value::Text(fs::read_to_string(path).unwrap().trim().to_string()),
        ),
    ])
}

fn key_row(path: &str) -> Row {
    HashMap::from([(
        "k".into(),
        Value::Text(
            Path::new(path)
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .into(),
        ),
    )])
}

fn tree(dirs: &[&str]) -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for d in dirs {
        fs::create_dir(root.join(d)).unwrap();
    }
    (dir, root)
}

/// Apply `event` on another thread and, once the batch is under way, ask
/// `sql` from this one.
fn query_mid_batch(db: &DirSQL, started: &Receiver<()>, event: FileEvent, sql: &str) -> Vec<Row> {
    let batch = {
        let db = db.clone();
        thread::spawn(move || db.apply_file_events(vec![event]))
    };
    started.recv_timeout(Duration::from_secs(30)).unwrap();
    let rows = db.query(sql).unwrap();
    batch.join().unwrap();
    rows
}

#[test]
fn a_query_during_a_batch_sees_every_table_before_it_or_after_it() {
    let (_dir, root) = tree(&["d"]);
    fs::write(root.join("d").join("1.txt"), "one").unwrap();
    let (pacer, started) = Pacer::new("2.txt");
    let db = DirSQL::new(
        &root,
        vec![
            pacer.table("a", "CREATE TABLE a (line TEXT)", "d/*.txt", line_row),
            pacer.table("b", "CREATE TABLE b (line TEXT)", "d/*.txt", line_row),
        ],
    )
    .unwrap();
    db.set_fresh_reads(false);
    let added = root.join("d").join("2.txt");
    fs::write(&added, "two").unwrap();

    let rows = query_mid_batch(
        &db,
        &started,
        FileEvent::Created(added),
        "SELECT (SELECT count(*) FROM a) AS na, (SELECT count(*) FROM b) AS nb",
    );

    assert_eq!(rows[0]["na"], rows[0]["nb"], "torn read: {rows:?}");
}

#[test]
fn a_query_during_a_batch_never_sees_rows_the_batch_rolls_back() {
    let (_dir, root) = tree(&["c", "p"]);
    fs::write(root.join("p").join("p1.txt"), "").unwrap();
    fs::write(root.join("c").join("c1.txt"), "p1").unwrap();
    let (pacer, started) = Pacer::new("orphan.txt");
    let db = DirSQL::new(
        &root,
        vec![
            pacer.table(
                "child",
                "CREATE TABLE child (id TEXT PRIMARY KEY, parent TEXT REFERENCES parent(k))",
                "c/*.txt",
                child_row,
            ),
            pacer.table("pad", "CREATE TABLE pad (line TEXT)", "c/*.txt", line_row),
            pacer.table(
                "parent",
                "CREATE TABLE parent (k TEXT PRIMARY KEY)",
                "p/*.txt",
                key_row,
            ),
        ],
    )
    .unwrap();
    db.set_fresh_reads(false);
    let orphan = root.join("c").join("orphan.txt");
    fs::write(&orphan, "MISSING").unwrap();

    let during = query_mid_batch(
        &db,
        &started,
        FileEvent::Created(orphan),
        "SELECT id FROM child ORDER BY id",
    );

    let after = db.query("SELECT id FROM child ORDER BY id").unwrap();
    assert_eq!(after, during, "read rows the batch rolled back");
}
