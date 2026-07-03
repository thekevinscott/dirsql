//! Integration tests for the internal `_dirsql_internal_rows` bookkeeping
//! mirror (issue #359, epic #358, stage 1).
//!
//! Stage 1 keeps the injected `_dirsql_file_path` / `_dirsql_row_index`
//! columns authoritative and *dual-writes* the mapping alongside them. These
//! tests exercise the equivalence guarantee across file create / modify /
//! delete, and that a persisted cache round-trips the mapping intact.

use dirsql::db::{Db, INTERNAL_ROWS_TABLE};
use dirsql::{DirSQL, Table, Value};
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn row(id: &str) -> HashMap<String, Value> {
    HashMap::from([("id".into(), Value::Text(id.into()))])
}

/// The mapping-derived state must equal the column-derived state after every
/// create / modify / delete against the row store.
#[test]
fn equivalence_holds_across_create_modify_delete() {
    let db = Db::new().unwrap();
    db.create_table("CREATE TABLE t (id TEXT)").unwrap();

    // Create: two files, several rows each.
    for (i, r) in ["a0", "a1"].iter().enumerate() {
        db.insert_row("t", &row(r), "a.jsonl", i).unwrap();
    }
    db.insert_row("t", &row("b0"), "b.jsonl", 0).unwrap();
    db.check_row_mapping_equivalence("t").unwrap();

    // Modify a.jsonl: delete its rows, insert a different count.
    db.delete_rows_by_file("t", "a.jsonl").unwrap();
    for (i, r) in ["a0'", "a1'", "a2'"].iter().enumerate() {
        db.insert_row("t", &row(r), "a.jsonl", i).unwrap();
    }
    db.check_row_mapping_equivalence("t").unwrap();

    // Delete b.jsonl entirely.
    db.delete_rows_by_file("t", "b.jsonl").unwrap();
    db.check_row_mapping_equivalence("t").unwrap();

    // The surviving mapping describes exactly the surviving user rows.
    let mapping_count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM _dirsql_internal_rows WHERE table_name='t'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mapping_count, 3);
}

/// A persisted cache written by a real `DirSQL` build must contain a mapping
/// that round-trips: reopening the cache and running the equivalence check
/// passes for every table.
#[test]
fn persisted_cache_round_trips_the_mapping() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\nalpha\nbeta\n").unwrap();
    fs::write(root.path().join("b.csv"), "col\ngamma\n").unwrap();

    let table = Table::new("CREATE TABLE rows (col TEXT)", "**/*.csv", |path| {
        let content = fs::read_to_string(path).unwrap();
        content
            .lines()
            .skip(1)
            .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
            .collect()
    });

    let db = DirSQL::builder()
        .root(root.path())
        .table(table)
        .persist(true)
        .build()
        .unwrap();
    // Three rows total across the two files.
    let rows = db.query("SELECT col FROM rows").unwrap();
    assert_eq!(rows.len(), 3);
    drop(db);

    // Reopen the on-disk cache and confirm the mapping matches the columns.
    let cache = root.path().join(".dirsql").join("cache.db");
    let reopened = Db::open(&cache).unwrap();
    reopened.check_row_mapping_equivalence("rows").unwrap();

    let mapping_count: i64 = reopened
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM _dirsql_internal_rows WHERE table_name='rows'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        mapping_count, 3,
        "mapping must survive the cache round-trip"
    );
}

/// The mapping table (and its by-file index) is a durable sidecar: it exists in
/// a freshly written cache file, discoverable via `sqlite_master`.
#[test]
fn mapping_table_is_a_durable_sidecar() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\nx\n").unwrap();

    let table = Table::new("CREATE TABLE rows (col TEXT)", "**/*.csv", |path| {
        let content = fs::read_to_string(path).unwrap();
        content
            .lines()
            .skip(1)
            .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
            .collect()
    });
    let db = DirSQL::builder()
        .root(root.path())
        .table(table)
        .persist(true)
        .build()
        .unwrap();
    drop(db);

    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    let exists = table_exists(&conn, INTERNAL_ROWS_TABLE);
    assert!(exists, "{INTERNAL_ROWS_TABLE} must persist in the cache");
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |r| r.get(0),
        )
        .unwrap();
    count > 0
}

/// A cache written by an older schema version (no mapping table) triggers a
/// full rebuild on the next startup, which repopulates the mapping.
#[test]
fn schema_bump_rebuilds_and_repopulates_mapping() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\none\ntwo\n").unwrap();

    let make_table = || {
        Table::new("CREATE TABLE rows (col TEXT)", "**/*.csv", |path| {
            let content = fs::read_to_string(path).unwrap();
            content
                .lines()
                .skip(1)
                .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
                .collect()
        })
    };

    // First build populates the cache under the current schema version.
    let db = DirSQL::builder()
        .root(root.path())
        .table(make_table())
        .persist(true)
        .build()
        .unwrap();
    drop(db);

    // Simulate an older cache: rewrite the stored schema version to "1" and
    // drop the mapping table, as a pre-#359 build would have left it.
    let cache = root.path().join(".dirsql").join("cache.db");
    {
        let conn = Connection::open(&cache).unwrap();
        conn.execute(
            "UPDATE _dirsql_meta SET value='1' WHERE key='schema_version'",
            [],
        )
        .unwrap();
        conn.execute("DROP TABLE _dirsql_internal_rows", [])
            .unwrap();
    }

    // Reopening forces a cold rebuild, which recreates and repopulates the map.
    let db = DirSQL::builder()
        .root(root.path())
        .table(make_table())
        .persist(true)
        .build()
        .unwrap();
    drop(db);

    let reopened = Db::open(&cache).unwrap();
    reopened.check_row_mapping_equivalence("rows").unwrap();
    let mapping_count: i64 = reopened
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM _dirsql_internal_rows WHERE table_name='rows'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mapping_count, 2);
}
