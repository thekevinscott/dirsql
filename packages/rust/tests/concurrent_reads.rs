//! A promoted database is one SQLite file that several connections share: a
//! read on one connection neither waits for nor is torn by a write committing
//! on another, and the file is gone once the last connection closes.

mod common;

use common::build_fixture_extension;
use dirsql::db::Db;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

const ROWS: i64 = 300_000;

const SLOW: &str = "SELECT sum(length(printf('%d-%s-%d', a, b, a * 7))) FROM t, \
     (SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4)";

fn database_file(db: &Db) -> PathBuf {
    let file: String = db
        .conn()
        .query_row(
            "SELECT file FROM pragma_database_list WHERE name = 'main'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        !file.is_empty(),
        "Db::promote must move the database to a named file other connections can share"
    );
    PathBuf::from(file)
}

fn populated() -> Db {
    let mut db = Db::new().unwrap();
    db.create_table("t", "CREATE TABLE t (a INTEGER, b TEXT)")
        .unwrap();
    db.conn()
        .execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < {ROWS}) \
             INSERT INTO t SELECT x, hex(randomblob(16)) FROM n"
        ))
        .unwrap();
    db.promote().unwrap();
    database_file(&db);
    db
}

fn count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM t", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn promotion_keeps_every_row() {
    let db = populated();
    assert_eq!(count(db.conn()), ROWS);
}

#[test]
fn a_write_and_a_cheap_read_finish_while_a_slow_read_runs_and_the_slow_read_is_not_torn() {
    let db = populated();
    let slow_reader = db.reader().unwrap();
    let cheap_reader = db.reader().unwrap();

    let slow_done = Arc::new(AtomicBool::new(false));
    let (pinned_tx, pinned_rx) = mpsc::channel();
    let slow = {
        let slow_done = Arc::clone(&slow_done);
        thread::spawn(move || {
            let conn = slow_reader.conn();
            conn.execute_batch("BEGIN").unwrap();
            let before = count(conn);
            pinned_tx.send(()).unwrap();
            let _: i64 = conn.query_row(SLOW, [], |row| row.get(0)).unwrap();
            let after = count(conn);
            conn.execute_batch("COMMIT").unwrap();
            slow_done.store(true, Ordering::SeqCst);
            (before, after)
        })
    };

    pinned_rx.recv().unwrap();
    db.conn()
        .execute("INSERT INTO t VALUES (-1, 'x')", [])
        .unwrap();
    let seen = count(cheap_reader.conn());
    let still_running = !slow_done.load(Ordering::SeqCst);

    let (before, after) = slow.join().unwrap();
    assert!(
        still_running,
        "the write and the cheap read waited for the slow read to finish"
    );
    assert_eq!(seen, ROWS + 1, "the cheap read missed the committed write");
    assert_eq!(
        (before, after),
        (ROWS, ROWS),
        "the slow read saw a write commit partway through it"
    );
}

fn leftovers(path: &Path) -> Vec<PathBuf> {
    ["", "-journal", "-wal", "-shm"]
        .iter()
        .map(|suffix| PathBuf::from(format!("{}{suffix}", path.display())))
        .filter(|file| file.exists())
        .collect()
}

#[test]
fn the_database_files_are_deleted_once_the_last_connection_closes() {
    let db = populated();
    let path = database_file(&db);
    let reader = db.reader().unwrap();
    assert!(path.exists(), "the database file is missing while open");

    drop(db);
    assert!(
        path.exists(),
        "the database file went while a reader had it open"
    );
    drop(reader);

    assert_eq!(leftovers(&path), Vec::<PathBuf>::new());
}

#[test]
fn the_writer_and_a_reader_have_the_extensions_loaded_before_promotion() {
    let ext = build_fixture_extension();
    let mut db = Db::new().unwrap();
    db.load_extension(&ext, None).unwrap();
    db.promote().unwrap();
    database_file(&db);

    let reader = db.reader().unwrap();

    for conn in [db.conn(), reader.conn()] {
        let answer: i64 = conn
            .query_row("SELECT dirsql_testext_answer()", [], |row| row.get(0))
            .unwrap();
        assert_eq!(answer, 42);
    }
}
