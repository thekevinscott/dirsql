//! Gap-filling tests for features documented in docs/ but previously untested
//! on the Rust SDK side.
//!
//! Each test cites the canonical doc location (docs page + section) it covers.
//! These mirror `packages/python/tests/integration/test_docs_gaps.py` for the
//! Rust SDK (bead dirsql-9ng). See TESTS_AUDIT.md.

use dirsql_sdk::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// docs/guide/config.md -- "Supported Formats" (.tsv/.ndjson/.toml/.yaml/.yml/.md)
// ---------------------------------------------------------------------------

/// Docs (guide/config.md "Supported Formats"): .tsv format is tab-separated.
#[test]
fn from_config_loads_tsv_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE produce (name TEXT, count TEXT)"
glob = "*.tsv"
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("data.tsv"),
        "name\tcount\napples\t10\noranges\t20\n",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT name, count FROM produce ORDER BY name")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("apples".into()));
    assert_eq!(rows[0]["count"], Value::Text("10".into()));
    assert_eq!(rows[1]["name"], Value::Text("oranges".into()));
}

/// Docs (guide/config.md "Supported Formats"): .ndjson aliases JSONL (one row per line).
#[test]
fn from_config_loads_ndjson_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE events (type TEXT, count INTEGER)"
glob = "*.ndjson"
"#,
    )
    .unwrap();
    fs::write(
        root.path().join("events.ndjson"),
        "{\"type\":\"click\",\"count\":5}\n{\"type\":\"view\",\"count\":100}\n",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT type, count FROM events ORDER BY type")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["type"], Value::Text("click".into()));
    assert_eq!(rows[0]["count"], Value::Integer(5));
    assert_eq!(rows[1]["type"], Value::Text("view".into()));
    assert_eq!(rows[1]["count"], Value::Integer(100));
}

/// Docs (guide/config.md "Supported Formats"): .toml data files produce one row per file.
#[test]
fn from_config_loads_toml_data_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE app (name TEXT, version TEXT)"
glob = "config/*.toml"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("config")).unwrap();
    fs::write(
        root.path().join("config").join("app.toml"),
        "name = \"myapp\"\nversion = \"1.2\"\n",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT name, version FROM app").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("myapp".into()));
    assert_eq!(rows[0]["version"], Value::Text("1.2".into()));
}

/// Docs (guide/config.md "Supported Formats"): .yaml mapping = 1 row.
#[test]
fn from_config_loads_yaml_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "*.yaml"
"#,
    )
    .unwrap();
    fs::write(root.path().join("data.yaml"), "name: widget\nprice: 9.99\n").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT name, price FROM items").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
    match &rows[0]["price"] {
        Value::Real(v) => assert!((v - 9.99).abs() < 1e-9, "price was {v}"),
        other => panic!("expected Real, got {other:?}"),
    }
}

/// Docs (guide/config.md "Supported Formats"): .yml is equivalent to .yaml.
#[test]
fn from_config_loads_yml_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "*.yml"
"#,
    )
    .unwrap();
    fs::write(root.path().join("data.yml"), "name: widget\nprice: 9.99\n").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
}

/// Docs (guide/config.md "Supported Formats"): .md uses YAML frontmatter + body column.
#[test]
fn from_config_loads_markdown_with_frontmatter() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE posts (title TEXT, author TEXT)"
glob = "posts/*.md"
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::write(
        root.path().join("posts").join("hello.md"),
        "---\ntitle: Hello\nauthor: Alice\n---\nBody text here.\n",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT title, author FROM posts").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("Hello".into()));
    assert_eq!(rows[0]["author"], Value::Text("Alice".into()));
}

// ---------------------------------------------------------------------------
// docs/guide/config.md -- "Strict Mode" (strict = true)
// ---------------------------------------------------------------------------

/// Docs (guide/config.md "Strict Mode"): `strict = true` errors on extra keys.
#[test]
fn from_config_strict_true_rejects_extra_keys() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "items/*.json"
strict = true
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("items")).unwrap();
    fs::write(
        root.path().join("items").join("a.json"),
        r#"{"name": "apple", "color": "red"}"#,
    )
    .unwrap();

    // Either construction fails, or the first query surfaces the strict violation.
    let result =
        DirSQL::from_config(root.path()).and_then(|db| db.query("SELECT * FROM items").map(|_| ()));
    assert!(
        result.is_err(),
        "expected strict=true to reject extra keys, got Ok"
    );
}

/// Docs (guide/config.md "Strict Mode"): strict mode passes on exact key match.
#[test]
fn from_config_strict_true_allows_exact_match() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT, color TEXT)"
glob = "items/*.json"
strict = true
"#,
    )
    .unwrap();
    fs::create_dir_all(root.path().join("items")).unwrap();
    fs::write(
        root.path().join("items").join("a.json"),
        r#"{"name": "apple", "color": "red"}"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
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
        move |_path, _content| {
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
    use dirsql_sdk::DirSQL;
    use futures_executor::block_on;
    use futures_util::StreamExt;
    use std::time::Duration;

    let root = TempDir::new().unwrap();
    let table = Table::new(
        "CREATE TABLE items (name TEXT)",
        "**/*.txt",
        |_, content| {
            vec![HashMap::from([(
                "name".into(),
                Value::Text(content.trim().to_string()),
            )])]
        },
    );
    let db = DirSQL::new(root.path(), vec![table]).unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("new_item.txt"), "apple").unwrap();

    let event = block_on(stream.next()).expect("expected watch event");
    match event {
        dirsql_sdk::RowEvent::Insert {
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
