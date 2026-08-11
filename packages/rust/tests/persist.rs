//! Integration tests for the persistent on-disk SQLite cache.
//!
//! These tests exercise the contract described in
//! `docs/howto/persist.md`: a warm start with an unchanged tree must
//! produce the same rows as a cold rebuild, while skipping the extract step
//! for files whose filesystem metadata matches the cache.

use dirsql::{DirSQL, DirSqlError, Row, Table, Value};
use rusqlite;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

/// Returns a CSV table whose extract function increments `counter` every time
/// it runs. Used to verify that warm starts skip extract for unchanged files.
fn counting_csv_table(counter: Arc<AtomicUsize>) -> Table {
    Table::new("CREATE TABLE rows (col TEXT)", "**/*.csv", move |path| {
        let content = std::fs::read_to_string(path).unwrap();
        counter.fetch_add(1, Ordering::SeqCst);
        content
            .lines()
            .skip(1) // header
            .map(|line| HashMap::from([("col".into(), Value::Text(line.trim().to_string()))]))
            .collect::<Vec<Row>>()
    })
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
        .persist(None::<&Path>)
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

#[test]
fn cold_start_writes_cache_at_default_path() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    let _db = open(root.path(), counter);

    let cache = root.path().join(".dirsql").join("cache.db");
    assert!(
        cache.exists(),
        "expected cache at default .dirsql/cache.db path"
    );
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
        .persist(Some(&custom))
        .build()
        .unwrap();

    assert!(custom.exists(), "expected cache at the custom persist_path");
    assert!(
        !root.path().join(".dirsql").join("cache.db").exists(),
        "default path should not be created when persist_path is set",
    );
}

#[test]
fn persist_with_unopenable_path_errors() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    // A cache path "inside" a regular file cannot have its parent created.
    let blocker = root.path().join("blocker");
    fs::write(&blocker, b"x").unwrap();
    let bad_cache = blocker.join("nested").join("cache.db");

    let counter = Arc::new(AtomicUsize::new(0));
    let result = DirSQL::builder()
        .root(root.path())
        .table(counting_csv_table(counter))
        .persist(Some(&bad_cache))
        .build();
    let err = match result {
        Ok(_) => panic!("expected an error when the persist path's parent is a file"),
        Err(e) => e,
    };
    assert!(matches!(err, DirSqlError::Io(_)), "got: {err}");
}

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

#[test]
fn warm_start_reparses_modified_file() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }

    // Wait past 1-second filesystem timestamp resolution so the modified
    // mtime is distinguishable from the cached snapshot.
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

    // A different glob set mismatches the cached glob_config_hash.
    let csv_counter = Arc::new(AtomicUsize::new(0));
    let tsv_counter = Arc::new(AtomicUsize::new(0));
    let csv_table = counting_csv_table(csv_counter.clone());
    let tsv_table = Table::new("CREATE TABLE tsv_rows (col TEXT)", "**/*.tsv", {
        let c = tsv_counter.clone();
        move |path| {
            let content = std::fs::read_to_string(path).unwrap();
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
        .persist(None::<&Path>)
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
// Fan-out under persistence: a file matching two tables is bookkept per
// (rel_path, table_name); the composite key round-trips across runs (#580).
// ---------------------------------------------------------------------------

/// A counting table over `**/*.csv` with a distinct name/column.
fn counting_named_table(name: &'static str, col: &'static str, counter: Arc<AtomicUsize>) -> Table {
    Table::new(
        &format!("CREATE TABLE {name} ({col} TEXT)"),
        "**/*.csv",
        move |path| {
            let content = std::fs::read_to_string(path).unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
            content
                .lines()
                .skip(1)
                .map(|line| HashMap::from([(col.into(), Value::Text(line.trim().to_string()))]))
                .collect::<Vec<Row>>()
        },
    )
}

fn open_two(root: &Path, ca: Arc<AtomicUsize>, cb: Arc<AtomicUsize>) -> DirSQL {
    DirSQL::builder()
        .root(root)
        .tables(vec![
            counting_named_table("ta", "col_a", ca),
            counting_named_table("tb", "col_b", cb),
        ])
        .persist(None::<&Path>)
        .build()
        .unwrap()
}

#[test]
fn persist_fans_out_and_composite_key_round_trips() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let ca = Arc::new(AtomicUsize::new(0));
    let cb = Arc::new(AtomicUsize::new(0));
    {
        let db = open_two(root.path(), ca.clone(), cb.clone());
        assert_eq!(db.query("SELECT col_a FROM ta").unwrap().len(), 1);
        assert_eq!(
            db.query("SELECT col_b FROM tb").unwrap().len(),
            1,
            "second-declared table populated on cold start"
        );
    }
    assert_eq!(ca.swap(0, Ordering::SeqCst), 1, "ta extracted once");
    assert_eq!(cb.swap(0, Ordering::SeqCst), 1, "tb extracted once");

    // Warm start over the same root/cache: both (rel_path, table) entries are
    // trusted, so neither table re-extracts, yet both still serve the row.
    let db = open_two(root.path(), ca.clone(), cb.clone());
    assert_eq!(ca.load(Ordering::SeqCst), 0, "ta trusted on warm start");
    assert_eq!(cb.load(Ordering::SeqCst), 0, "tb trusted on warm start");
    assert_eq!(db.query("SELECT col_a FROM ta").unwrap().len(), 1);
    assert_eq!(db.query("SELECT col_b FROM tb").unwrap().len(), 1);
}

#[test]
fn persist_cache_records_a_row_per_matching_table() {
    use rusqlite::Connection;

    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let ca = Arc::new(AtomicUsize::new(0));
    let cb = Arc::new(AtomicUsize::new(0));
    {
        let _db = open_two(root.path(), ca, cb);
    }

    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    let files: i64 = conn
        .query_row("SELECT COUNT(*) FROM _dirsql_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        files, 2,
        "one file matching two tables must record two (rel_path, table) rows"
    );
}

#[test]
fn old_schema_version_cache_is_rebuilt() {
    use rusqlite::Connection;

    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }
    counter.store(0, Ordering::SeqCst);

    // Force the cache to the pre-fan-out schema version. The bumped version
    // must make this cache incompatible, triggering a full rebuild.
    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    conn.execute(
        "UPDATE _dirsql_meta SET value = '3' WHERE key = 'schema_version'",
        [],
    )
    .unwrap();
    drop(conn);

    let db = open(root.path(), counter.clone());
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "an old-schema-version cache must be discarded and rebuilt",
    );
    assert_eq!(db.query("SELECT col FROM rows").unwrap().len(), 1);
}

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

/// A hook failure no longer discards the scan. The files that parsed are
/// committed to the cache, and the file that failed is absent from
/// `_dirsql_files` -- so the cache is incomplete, never wrong, and the missing
/// file is retried on the next scan rather than being remembered as done.
///
/// This is the persist-side half of dirsql#714: the all-or-nothing transaction
/// was protecting the assumption that every scanned file has an index entry by
/// commit time, and that assumption is what a partial commit has to survive.
#[test]
fn a_hook_failure_commits_the_files_that_parsed() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);
    write_csv(root.path(), "b.csv", &["beta"]);
    write_csv(root.path(), "c.csv", &["gamma"]);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_cb = Arc::clone(&counter);
    let db = DirSQL::builder()
        .root(root.path())
        .table(Table::try_new(
            "CREATE TABLE rows (col TEXT)",
            "**/*.csv",
            move |path| {
                let content = std::fs::read_to_string(path).unwrap();
                let count = counter_cb.fetch_add(1, Ordering::SeqCst);
                if count == 2 {
                    // Fail on the 3rd file (count starts at 0)
                    return Err("boom".into());
                }
                Ok(content
                    .lines()
                    .skip(1)
                    .map(|line| {
                        HashMap::from([("col".into(), Value::Text(line.trim().to_string()))])
                    })
                    .collect::<Vec<Row>>())
            },
        ))
        .persist(None::<&Path>)
        .build()
        .expect("one file's hook failure must not discard the scan");

    let failures = db.scan_failures();
    assert_eq!(failures.len(), 1, "expected one skipped file: {failures:?}");
    drop(db);

    // Open the cache with a raw connection to verify what was committed.
    let cache_path = root.path().join(".dirsql").join("cache.db");
    let cache_conn = rusqlite::Connection::open(&cache_path).unwrap();

    let row_count: i64 = cache_conn
        .query_row("SELECT COUNT(*) FROM rows", [], |r| r.get(0))
        .unwrap();
    assert_eq!(row_count, 2, "the two files that parsed should persist");

    // The failed file never reached `upsert_file`, so a later scan sees it as
    // unknown and retries it instead of trusting a stale entry.
    let file_count: i64 = cache_conn
        .query_row("SELECT COUNT(*) FROM _dirsql_files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        file_count, 2,
        "only the files that parsed belong in the index"
    );
}

#[test]
fn warm_start_over_an_unchanged_tree_leaves_the_cache_untouched() {
    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);
    write_csv(root.path(), "b.csv", &["beta"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter.clone());
    }

    let cache = root.path().join(".dirsql").join("cache.db");
    let size_before = fs::metadata(&cache).unwrap().len();
    let digest_before = blake3::hash(&fs::read(&cache).unwrap());

    {
        let _db = open(root.path(), counter.clone());
    }

    assert_eq!(counter.load(Ordering::SeqCst), 2, "cold parses, warm skips");
    assert_eq!(
        fs::metadata(&cache).unwrap().len(),
        size_before,
        "an unchanged tree must not grow the cache",
    );
    assert_eq!(
        blake3::hash(&fs::read(&cache).unwrap()),
        digest_before,
        "an unchanged tree must not rewrite the cache",
    );
}

#[test]
fn persist_cache_uses_wal_journal_mode() {
    use rusqlite::Connection;

    let root = TempDir::new().unwrap();
    write_csv(root.path(), "a.csv", &["alpha"]);

    let counter = Arc::new(AtomicUsize::new(0));
    {
        let _db = open(root.path(), counter);
    }

    let cache = root.path().join(".dirsql").join("cache.db");
    let conn = Connection::open(&cache).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "cache must use WAL journal mode");
}
