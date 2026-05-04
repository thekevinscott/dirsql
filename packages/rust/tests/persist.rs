//! Integration tests for the persistent on-disk SQLite cache (issue #95).
//!
//! These tests exercise the contract described in
//! `docs/guide/persistence.md`: a warm start with an unchanged tree must
//! produce the same rows as a cold rebuild, while skipping the extract step
//! for files whose filesystem metadata matches the cache.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

/// Returns a CSV table whose extract function increments `counter` every time
/// it runs. Used to verify that warm starts skip extract for unchanged files.
fn counting_csv_table(counter: Arc<AtomicUsize>) -> Table {
    Table::new(
        "CREATE TABLE rows (col TEXT)",
        "**/*.csv",
        move |_path, content| {
            counter.fetch_add(1, Ordering::SeqCst);
            content
                .lines()
                .skip(1) // header
                .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
                .collect::<Vec<Row>>()
        },
    )
}

fn write_csv(root: &Path, name: &str, body_lines: &[&str]) {
    let mut content = String::from("col\n");
    for line in body_lines {
        content.push_str(line);
        content.push('\n');
    }
    fs::write(root.join(name), content).unwrap();
}

fn open(root: &Path, counter: Arc<AtomicUsize>) -> DirSQL {
    DirSQL::builder()
        .root(root)
        .table(counting_csv_table(counter))
        .persist(true)
        .build()
        .unwrap()
}

fn open_in_memory(root: &Path, counter: Arc<AtomicUsize>) -> DirSQL {
    DirSQL::builder()
        .root(root)
        .table(counting_csv_table(counter))
        .build()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Cold-start: cache file is created at the documented default path.
// ---------------------------------------------------------------------------

#[test]
fn cold_start_writes_cache_at_default_path() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    let _db = open(root.path(), counter);

    let cache = root.path().join(".dirsql").join("cache.db");
    assert!(cache.exists(), "expected cache at {}", cache.display());
}

#[test]
fn custom_persist_path_is_honored() {
    let root = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let custom = cache_dir.path().join("nested").join("my-cache.db");
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    let _db = DirSQL::builder()
        .root(root.path())
        .table(counting_csv_table(counter))
        .persist(true)
        .persist_path(&custom)
        .build()
        .unwrap();

    assert!(custom.exists(), "expected cache at {}", custom.display());
    assert!(
        !root.path().join(".dirsql").join("cache.db").exists(),
        "default path should not be created when persist_path is set",
    );
}

// ---------------------------------------------------------------------------
// Warm-start: extract is NOT called for unchanged files; rows are equivalent.
// ---------------------------------------------------------------------------

#[test]
fn warm_start_skips_extract_for_unchanged_files() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);
    write_csv(root.path(), "b.csv", &["beta"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }
    let cold = counter.swap(0, Ordering::SeqCst);
    assert_eq!(cold, 2, "cold start should extract once per file");

    let db = open(root.path(), counter.clone());
    let warm = counter.load(Ordering::SeqCst);
    assert_eq!(
        warm, 0,
        "warm start with unchanged files must not invoke extract",
    );

    let rows = db.query("SELECT col FROM rows ORDER BY col").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["col"], Value::Text("alpha".into()));
    assert_eq!(rows[1]["col"], Value::Text("beta".into()));
}

#[test]
fn warm_start_returns_same_rows_as_cold_rebuild() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha", "alpha2"]);
    write_csv(root.path(), "b.csv", &["beta"]);

    let counter = Arc::new(AtomicUsize::new(0));
    let cold = open_in_memory(root.path(), counter.clone());
    let cold_rows = cold.query("SELECT col FROM rows ORDER BY col").unwrap();

    {
        let _seed = open(root.path(), counter.clone());
    }
    let warm = open(root.path(), counter);
    let warm_rows = warm.query("SELECT col FROM rows ORDER BY col").unwrap();

    assert_eq!(cold_rows, warm_rows);
}

// ---------------------------------------------------------------------------
// Warm-start with changes: changes are picked up.
// ---------------------------------------------------------------------------

#[test]
fn warm_start_reparses_modified_file() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }

    // Wait long enough for any 1-second filesystem timestamp resolution to
    // distinguish the modified mtime from the cached snapshot.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_csv(root.path(), "a.csv", &["alpha-updated"]);

    counter.store(0, Ordering::SeqCst);
    let db = open(root.path(), counter.clone());

    assert!(
        counter.load(Ordering::SeqCst) >= 1,
        "modified file must be re-parsed",
    );
    let rows = db.query("SELECT col FROM rows").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["col"], Value::Text("alpha-updated".into()));
}

#[test]
fn warm_start_drops_rows_for_deleted_file() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);
    write_csv(root.path(), "b.csv", &["beta"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }

    fs::remove_file(root.path().join("b.csv")).unwrap();

    let db = open(root.path(), counter);
    let rows = db.query("SELECT col FROM rows ORDER BY col").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["col"], Value::Text("alpha".into()));
}

#[test]
fn warm_start_ingests_new_file() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }

    write_csv(root.path(), "b.csv", &["beta"]);

    let db = open(root.path(), counter);
    let rows = db.query("SELECT col FROM rows ORDER BY col").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["col"], Value::Text("alpha".into()));
    assert_eq!(rows[1]["col"], Value::Text("beta".into()));
}

// ---------------------------------------------------------------------------
// Full-rebuild triggers: glob change, dirsql_version bump, etc.
// ---------------------------------------------------------------------------

#[test]
fn glob_config_change_forces_full_rebuild() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);
    fs::write(root.path().join("a.tsv"), "col\nalpha-tsv\n").unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }
    let cold = counter.swap(0, Ordering::SeqCst);
    assert_eq!(cold, 1);

    // Change the glob: now match *.tsv too. This is a different glob set, so
    // the cached glob_config_hash should mismatch and force a full rebuild.
    let csv_counter = Arc::new(AtomicUsize::new(0));
    let tsv_counter = Arc::new(AtomicUsize::new(0));
    let csv_table = counting_csv_table(csv_counter.clone());
    let tsv_table = Table::new("CREATE TABLE tsv_rows (col TEXT)", "**/*.tsv", {
        let c = tsv_counter.clone();
        move |_path, content| {
            c.fetch_add(1, Ordering::SeqCst);
            content
                .lines()
                .skip(1)
                .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
                .collect::<Vec<Row>>()
        }
    });

    let db = DirSQL::builder()
        .root(root.path())
        .tables(vec![csv_table, tsv_table])
        .persist(true)
        .build()
        .unwrap();

    assert_eq!(
        csv_counter.load(Ordering::SeqCst),
        1,
        "glob change must trigger full rebuild (csv re-parsed)",
    );
    assert_eq!(
        tsv_counter.load(Ordering::SeqCst),
        1,
        "glob change must trigger full rebuild (tsv parsed for first time)",
    );

    let rows = db.query("SELECT col FROM tsv_rows").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["col"], Value::Text("alpha-tsv".into()));
}

#[test]
fn corrupted_meta_triggers_full_rebuild() {
    use rusqlite::Connection;

    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }
    counter.store(0, Ordering::SeqCst);

    // Manually corrupt the cached dirsql_version.
    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    conn.execute(
        "UPDATE _dirsql_meta SET value = 'bogus-version' WHERE key = 'dirsql_version'",
        [],
    )
    .unwrap();
    drop(conn);

    let db = open(root.path(), counter.clone());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "version mismatch must trigger full rebuild",
    );
    let rows = db.query("SELECT col FROM rows").unwrap();
    assert_eq!(rows.len(), 1);
}

// ---------------------------------------------------------------------------
// .dirsql/ exclusion (must hold whether persist is on or off).
// ---------------------------------------------------------------------------

#[test]
fn dirsql_directory_excluded_when_persist_enabled() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "real.csv", &["alpha"]);

    fs::create_dir_all(root.path().join(".dirsql")).unwrap();
    write_csv(
        &root.path().join(".dirsql"),
        "junk.csv",
        &["should-not-appear"],
    );

    let counter = Arc::new(AtomicUsize::new(0));
    let db = open(root.path(), counter);

    let rows = db.query("SELECT col FROM rows ORDER BY col").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["col"], Value::Text("alpha".into()));
}

#[test]
fn dirsql_directory_excluded_when_persist_disabled() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "real.csv", &["alpha"]);

    fs::create_dir_all(root.path().join(".dirsql")).unwrap();
    write_csv(
        &root.path().join(".dirsql"),
        "junk.csv",
        &["should-not-appear"],
    );

    let counter = Arc::new(AtomicUsize::new(0));
    let db = open_in_memory(root.path(), counter);

    let rows = db.query("SELECT col FROM rows ORDER BY col").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["col"], Value::Text("alpha".into()));
}

// ---------------------------------------------------------------------------
// Sidecar tables exist and are populated.
// ---------------------------------------------------------------------------

#[test]
fn cache_contains_sidecar_tables() {
    use rusqlite::Connection;

    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter);
    }

    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();

    let files: i64 = conn
        .query_row("SELECT COUNT(*) FROM _dirsql_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(files, 1, "_dirsql_files should have one row");

    let meta_keys: Vec<String> = conn
        .prepare("SELECT key FROM _dirsql_meta ORDER BY key")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    for required in &[
        "dirsql_version",
        "glob_config_hash",
        "root_canonical",
        "schema_version",
    ] {
        assert!(
            meta_keys.iter().any(|k| k == required),
            "_dirsql_meta missing key {required}; found: {meta_keys:?}",
        );
    }
}
