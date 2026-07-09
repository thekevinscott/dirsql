//! Integration tests for the internal `_dirsql_internal_rows` bookkeeping
//! table. The mapping is the sole record of row ownership — user tables carry
//! no injected tracking columns — so these tests assert the mapping stays
//! consistent with the live user rows across create / modify / delete and
//! round-trips the persisted cache.

use dirsql::db::{Db, INTERNAL_ROWS_TABLE};
use dirsql::{DirSQL, Table, Value};
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn row(id: &str) -> HashMap<String, Value> {
    HashMap::from([("id".into(), Value::Text(id.into()))])
}

/// Assert the mapping for `table` has exactly `expected` rows, the user table
/// has `expected` rows, and every mapping `rowid_ref` points at a live row.
fn assert_mapping_consistent(db: &Db, table: &str, expected: i64) {
    let mapping: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM _dirsql_internal_rows WHERE table_name = ?1",
            [table],
            |r| r.get(0),
        )
        .unwrap();
    let user: i64 = db
        .conn()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap();
    let orphans: i64 = db
        .conn()
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM _dirsql_internal_rows m WHERE m.table_name = ?1 \
                 AND NOT EXISTS (SELECT 1 FROM {table} t WHERE t.rowid = m.rowid_ref)"
            ),
            [table],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mapping, expected, "mapping row count");
    assert_eq!(user, expected, "user row count");
    assert_eq!(orphans, 0, "mapping has orphaned rowid_refs");
}

/// The mapping stays consistent with the live user rows across every
/// create / modify / delete against the row store.
#[test]
fn mapping_tracks_create_modify_delete() {
    let db = Db::new().unwrap();
    db.create_table("CREATE TABLE t (id TEXT)").unwrap();

    for (i, r) in ["a0", "a1"].iter().enumerate() {
        db.insert_row("t", &row(r), "a.jsonl", i).unwrap();
    }
    db.insert_row("t", &row("b0"), "b.jsonl", 0).unwrap();
    assert_mapping_consistent(&db, "t", 3);

    db.delete_rows_by_file("t", "a.jsonl").unwrap();
    for (i, r) in ["a0'", "a1'", "a2'"].iter().enumerate() {
        db.insert_row("t", &row(r), "a.jsonl", i).unwrap();
    }
    assert_mapping_consistent(&db, "t", 4);

    db.delete_rows_by_file("t", "b.jsonl").unwrap();
    assert_mapping_consistent(&db, "t", 3);
}

/// A persisted cache written by a real `DirSQL` build round-trips the mapping.
#[test]
fn persisted_cache_round_trips_the_mapping() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\nalpha\nbeta\n").unwrap();
    fs::write(root.path().join("b.csv"), "col\ngamma\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .table(csv_table())
        .persist(None::<&std::path::Path>)
        .build()
        .unwrap();
    assert_eq!(db.query("SELECT col FROM rows").unwrap().len(), 3);
    drop(db);

    let cache = root.path().join(".dirsql").join("cache.db");
    let reopened = Db::open(&cache).unwrap();
    assert_mapping_consistent(&reopened, "rows", 3);
}

/// The mapping table is a durable sidecar: it exists in a freshly written cache
/// file, discoverable via `sqlite_master`.
#[test]
fn mapping_table_is_a_durable_sidecar() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\nx\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .table(csv_table())
        .persist(None::<&std::path::Path>)
        .build()
        .unwrap();
    drop(db);

    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    assert!(
        table_exists(&conn, INTERNAL_ROWS_TABLE),
        "{INTERNAL_ROWS_TABLE} must persist in the cache"
    );
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

fn csv_table() -> Table {
    Table::new("CREATE TABLE rows (col TEXT)", "**/*.csv", |path| {
        let content = fs::read_to_string(path).unwrap();
        content
            .lines()
            .skip(1)
            .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
            .collect()
    })
}

/// A cache written by an older schema version triggers a full rebuild on the
/// next startup, which recreates and repopulates the mapping.
#[test]
fn schema_bump_rebuilds_and_repopulates_mapping() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "col\none\ntwo\n").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .table(csv_table())
        .persist(None::<&std::path::Path>)
        .build()
        .unwrap();
    drop(db);

    let cache = root.path().join(".dirsql").join("cache.db");
    {
        let conn = Connection::open(&cache).unwrap();
        conn.execute(
            "UPDATE _dirsql_meta SET value='0' WHERE key='schema_version'",
            [],
        )
        .unwrap();
        conn.execute("DROP TABLE _dirsql_internal_rows", [])
            .unwrap();
    }

    let db = DirSQL::builder()
        .root(root.path())
        .table(csv_table())
        .persist(None::<&std::path::Path>)
        .build()
        .unwrap();
    drop(db);

    let reopened = Db::open(&cache).unwrap();
    assert_mapping_consistent(&reopened, "rows", 2);
}
