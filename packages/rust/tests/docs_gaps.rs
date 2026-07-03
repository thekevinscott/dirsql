//! Gap-filling tests for features documented in docs/ but previously untested
//! on the Rust SDK side.
//!
//! Each test cites the canonical doc location (docs page + section) it covers.
//! These mirror `packages/python/tests/binding/docs_gaps_test.py` for the
//! Rust SDK (bead dirsql-9ng). See TESTS_AUDIT.md.

use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// docs/guide/tables.md -- "Strict Mode" (strict = true on programmatic tables)
// ---------------------------------------------------------------------------

/// Docs (guide/tables.md "Strict Mode"): `strict = true` errors on extra keys
/// produced by the user extract.
#[test]
fn strict_true_rejects_extra_keys_from_extract() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.json"), "x").unwrap();

    let table = Table::strict("CREATE TABLE items (name TEXT)", "*.json", |_path| {
        vec![HashMap::from([
            ("name".into(), Value::Text("apple".into())),
            ("color".into(), Value::Text("red".into())),
        ])]
    });

    let result = DirSQL::new(root.path(), vec![table])
        .and_then(|db| db.query("SELECT * FROM items").map(|_| ()));
    assert!(
        result.is_err(),
        "expected strict=true to reject extra keys, got Ok"
    );
}

/// Docs (guide/tables.md "Strict Mode"): strict mode passes when the extract's
/// row keys match the DDL exactly.
#[test]
fn strict_true_allows_exact_match() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.json"), "x").unwrap();

    let table = Table::strict(
        "CREATE TABLE items (name TEXT, color TEXT)",
        "*.json",
        |_path| {
            vec![HashMap::from([
                ("name".into(), Value::Text("apple".into())),
                ("color".into(), Value::Text("red".into())),
            ])]
        },
    );

    let db = DirSQL::new(root.path(), vec![table]).unwrap();
    let rows = db.query("SELECT name, color FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert_eq!(rows[0]["color"], Value::Text("red".into()));
}

// ---------------------------------------------------------------------------
// docs/guide/tables.md -- "Supported value types" -> bytes -> BLOB
// ---------------------------------------------------------------------------

/// Docs (guide/tables.md "Supported value types"): Rust `Value::Blob` round-trips through SQLite BLOB.
#[test]
fn extract_blob_values_round_trip_via_sdk() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("marker.json"), "{}").unwrap();

    let payload: Vec<u8> = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
    let payload_for_closure = payload.clone();

    let table = Table::new(
        "CREATE TABLE blobs (name TEXT, data BLOB)",
        "*.json",
        move |_path| {
            vec![HashMap::from([
                ("name".into(), Value::Text("bin".into())),
                ("data".into(), Value::Blob(payload_for_closure.clone())),
            ])]
        },
    );

    let db = DirSQL::new(root.path(), vec![table]).unwrap();
    let rows = db.query("SELECT name, data FROM blobs").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("bin".into()));
    assert_eq!(rows[0]["data"], Value::Blob(payload));
}

// ---------------------------------------------------------------------------
// docs/guide/watching.md -- RowEvent.file_path relative-path assertion
// ---------------------------------------------------------------------------

/// Docs (guide/watching.md): Insert events carry `file_path`, the relative
/// path of the source file within the watched root.
#[test]
fn watch_insert_event_carries_relative_file_path() {
    use dirsql::DirSQL;
    use futures_executor::block_on;
    use futures_util::StreamExt;
    use std::time::Duration;

    let root = TempDir::new().unwrap();
    let table = Table::new("CREATE TABLE items (name TEXT)", "**/*.txt", |path| {
        let content = std::fs::read_to_string(path).unwrap();
        vec![HashMap::from([(
            "name".into(),
            Value::Text(content.trim().to_string()),
        )])]
    });
    let db = DirSQL::new(root.path(), vec![table]).unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("new_item.txt"), "apple").unwrap();

    let event = block_on(stream.next()).expect("expected watch event");
    match event {
        dirsql::RowEvent::Insert {
            table,
            row,
            file_path,
        } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
            // Must be a RELATIVE path, not absolute.
            assert!(
                !std::path::Path::new(&file_path).is_absolute(),
                "file_path should be relative, got: {file_path}"
            );
            // Normalize separators for cross-platform safety.
            let normalized = file_path.replace('\\', "/");
            assert_eq!(normalized, "new_item.txt");
        }
        other => panic!("expected insert event, got: {other:?}"),
    }
}
