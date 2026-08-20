//! Integration tests for `ddl` as a multi-statement SQL batch.
//!
//! A `[[table]]`'s `ddl` is handed to SQLite whole: any number of statements,
//! of any kind. dirsql never reads the text — what the batch produced is
//! settled afterwards against SQLite's own catalog. That is what makes
//! indexes, FTS5 virtual tables and triggers expressible in config.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const BASE: &str = "CREATE TABLE notes (path TEXT, body TEXT)";

/// A `notes` table plus an external-content FTS5 index kept current by an
/// insert and a delete trigger — the shape the config unlocks.
const FTS_DDL: &str = "\
CREATE TABLE notes (path TEXT, body TEXT);
CREATE VIRTUAL TABLE notes_fts USING fts5(body, content='notes', content_rowid='rowid');
CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
  INSERT INTO notes_fts(rowid, body) VALUES (new.rowid, new.body);
END;
CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
END;";

/// A `notes` table whose rows are each markdown file's name and contents.
fn notes_table(ddl: &str) -> Table {
    Table::new("notes", ddl, "*.md", |path| {
        let body = fs::read_to_string(path).unwrap();
        vec![HashMap::from([
            (
                "path".into(),
                Value::Text(
                    Path::new(path)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                ),
            ),
            ("body".into(), Value::Text(body.trim().to_string())),
        ])] as Vec<Row>
    })
}

fn seed(root: &Path) {
    fs::write(root.join("a.md"), "hello world").unwrap();
}

fn build(root: &Path, ddl: &str) -> dirsql::Result<DirSQL> {
    DirSQL::builder().root(root).table(notes_table(ddl)).build()
}

fn build_persist(root: &Path, ddl: &str) -> dirsql::Result<DirSQL> {
    DirSQL::builder()
        .root(root)
        .table(notes_table(ddl))
        .persist(None::<&Path>)
        .build()
}

/// The error message from a build that must fail. `DirSQL` is not `Debug`,
/// so `expect_err` is unavailable.
fn build_error(result: dirsql::Result<DirSQL>, what: &str) -> String {
    match result {
        Ok(_) => panic!("{what}"),
        Err(err) => err.to_string(),
    }
}

fn index_names(db: &DirSQL) -> Vec<Value> {
    db.query("SELECT name FROM pragma_index_list('notes')")
        .unwrap()
        .into_iter()
        .map(|row| row["name"].clone())
        .collect()
}

#[test]
fn a_multi_statement_batch_runs_every_statement() {
    let root = TempDir::new().unwrap();
    seed(root.path());

    let db = build(
        root.path(),
        &format!("{BASE};\nCREATE INDEX notes_path ON notes(path);"),
    )
    .expect("a multi-statement `ddl` must load");

    let rows = db.query("SELECT body FROM notes").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["body"], Value::Text("hello world".into()));

    let indexes = index_names(&db);
    assert!(
        indexes.contains(&Value::Text("notes_path".into())),
        "the batch's CREATE INDEX must have run, got {indexes:?}"
    );
}

/// The unlock the issue is really about: dirsql writes user rows with plain
/// `INSERT`/`DELETE`, so triggers declared in `ddl` keep an FTS5 index current
/// through the initial load *and* through a later file deletion.
#[test]
fn an_fts5_index_declared_in_ddl_tracks_inserts_and_deletes() {
    let root = TempDir::new().unwrap();
    seed(root.path());

    let db = build_persist(root.path(), FTS_DDL).expect("an FTS5 batch must load");
    let hits = db
        .query("SELECT body FROM notes_fts WHERE notes_fts MATCH 'hello'")
        .unwrap();
    assert_eq!(hits.len(), 1, "the insert trigger must have indexed the row");
    assert_eq!(hits[0]["body"], Value::Text("hello world".into()));
    drop(db);

    fs::remove_file(root.path().join("a.md")).unwrap();
    let db = build_persist(root.path(), FTS_DDL).expect("a warm rebuild must load");
    let hits = db
        .query("SELECT body FROM notes_fts WHERE notes_fts MATCH 'hello'")
        .unwrap();
    assert!(
        hits.is_empty(),
        "the delete trigger must have dropped the vanished file's row from the index, got {hits:?}"
    );
}

#[test]
fn a_batch_sqlite_rejects_fails_the_build_and_rolls_back() {
    let root = TempDir::new().unwrap();
    seed(root.path());

    let message = build_error(
        build_persist(root.path(), &format!("{BASE};\nCREATE TABLE oops (")),
        "a batch SQLite rejects must fail the build",
    );
    assert!(
        message.contains("table 'notes'"),
        "the error must carry the config-entry prefix `table 'notes'`, got {message:?}"
    );
    assert!(
        message.contains("incomplete input"),
        "SQLite's own error text must come through raw, got {message:?}"
    );

    let cache = rusqlite::Connection::open(root.path().join(".dirsql").join("cache.db")).unwrap();
    let created: i64 = cache
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_list WHERE schema = 'main' AND name = 'notes'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        created, 0,
        "the failed batch's earlier statements must roll back, leaving no `notes` table"
    );
}

/// A batch may create virtual tables freely — but the *declared* table is the
/// per-file row table dirsql inserts into, so it may not itself be one.
#[test]
fn a_declared_name_that_is_a_virtual_table_is_a_load_error() {
    let root = TempDir::new().unwrap();
    seed(root.path());

    let message = build_error(
        build(root.path(), "CREATE VIRTUAL TABLE notes USING fts5(path, body)"),
        "a dirsql table must be a real per-file row table",
    );
    assert!(
        message.contains("table 'notes'") && message.contains("virtual"),
        "the error must name the entry and say the table is virtual, got {message:?}"
    );
}

#[test]
fn the_not_created_error_names_what_the_batch_did_create() {
    let root = TempDir::new().unwrap();
    seed(root.path());

    let table = Table::new(
        "messages",
        "CREATE TABLE notes (path TEXT);\n\
         CREATE VIRTUAL TABLE notes_fts USING fts5(path);",
        "*.md",
        |_| Vec::<Row>::new(),
    );
    let message = build_error(
        DirSQL::builder().root(root.path()).table(table).build(),
        "a `name` the batch never creates must fail to load",
    );
    assert!(
        message.contains("table 'messages'"),
        "the error must carry the config-entry prefix, got {message:?}"
    );
    assert!(
        message.contains("notes") && message.contains("notes_fts"),
        "the error must list what the batch did create, got {message:?}"
    );
}

/// One invalidation lane: the persist hash covers the whole batch, so adding a
/// statement forces a full sweep and rebuild rather than a warm skip.
#[test]
fn editing_the_ddl_batch_rebuilds_the_cache() {
    let root = TempDir::new().unwrap();
    seed(root.path());
    drop(build_persist(root.path(), BASE).expect("first build"));

    let db = build_persist(
        root.path(),
        &format!("{BASE};\nCREATE INDEX notes_path ON notes(path);"),
    )
    .expect("an edited batch must rebuild");

    let indexes = index_names(&db);
    assert!(
        indexes.contains(&Value::Text("notes_path".into())),
        "a ddl edit must invalidate the cache and re-run the whole batch, got {indexes:?}"
    );
}

/// The sweep that precedes a rebuild has to cope with whatever the previous
/// batch created — virtual tables and the shadow tables behind them included.
#[test]
fn a_rebuild_sweeps_a_cache_holding_virtual_tables() {
    let root = TempDir::new().unwrap();
    seed(root.path());
    drop(build_persist(root.path(), FTS_DDL).expect("first build"));

    let db = build_persist(
        root.path(),
        &format!("{FTS_DDL}\nCREATE INDEX notes_path ON notes(path);"),
    )
    .expect("sweeping a cache holding a virtual table and its shadows must succeed");

    let hits = db
        .query("SELECT body FROM notes_fts WHERE notes_fts MATCH 'hello'")
        .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the rebuilt index must hold the re-ingested row, got {hits:?}"
    );
}
