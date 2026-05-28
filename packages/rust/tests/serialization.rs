//! Integration tests for the `DirSQL::config()` serialization method (issue #194).
//!
//! `DirSQL::config()` returns a `DirSQLConfig` struct capturing resolved
//! runtime state in a serde-serializable form:
//!
//! - `config` (the config-file path) is excluded -- by the time the instance
//!   exists the config file has been read and its contents merged into
//!   `root`, `tables`, and `ignore`.
//! - `extract` is excluded from the per-table shape -- closures are not
//!   serializable.
//! - `name` is excluded from the per-table shape.

use dirsql::{DirSQL, Table};
use std::path::PathBuf;
use tempfile::TempDir;

fn noop_table(ddl: &str, glob: &str) -> Table {
    Table::new(ddl, glob, |_| vec![])
}

#[test]
fn config_returns_resolved_state() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .build()
        .unwrap();

    let cfg = db.config();
    assert_eq!(cfg.root, root.path());
    assert_eq!(cfg.tables.len(), 1);
    assert_eq!(cfg.tables[0].ddl, "CREATE TABLE items (name TEXT)");
    assert_eq!(cfg.tables[0].glob, "items/*.json");
    assert!(!cfg.tables[0].strict);
    assert_eq!(cfg.ignore, Vec::<String>::new());
    assert!(!cfg.persist);
    assert_eq!(cfg.persist_path, None);
}

#[test]
fn config_serializes_to_json_value() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .build()
        .unwrap();

    let json = serde_json::to_value(db.config()).expect("config must be serde-serializable");

    let obj = json.as_object().expect("expected top-level object");
    assert!(obj.contains_key("root"));
    assert!(obj.contains_key("tables"));
    assert!(obj.contains_key("ignore"));
    assert!(obj.contains_key("persist"));
    assert!(obj.contains_key("persist_path"));

    let tables = obj.get("tables").unwrap().as_array().unwrap();
    assert_eq!(tables.len(), 1);
    let t = tables[0].as_object().unwrap();
    assert_eq!(
        t.get("ddl").and_then(|v| v.as_str()),
        Some("CREATE TABLE items (name TEXT)"),
    );
    assert_eq!(t.get("glob").and_then(|v| v.as_str()), Some("items/*.json"));
    assert_eq!(t.get("strict").and_then(|v| v.as_bool()), Some(false));
    assert!(!t.contains_key("extract"));
    assert!(!t.contains_key("name"));
}

#[test]
fn config_omits_extract_from_table_struct() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .build()
        .unwrap();

    let json = serde_json::to_value(db.config()).unwrap();
    let tables = json.get("tables").unwrap().as_array().unwrap();
    for t in tables {
        let obj = t.as_object().unwrap();
        assert!(
            !obj.contains_key("extract"),
            "extract must be excluded; closures are not serializable",
        );
        assert!(
            !obj.contains_key("name"),
            "name is not part of the serialized table shape",
        );
    }
}

#[test]
fn config_reflects_strict_true() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(Table::strict(
            "CREATE TABLE items (name TEXT)",
            "items/*.json",
            |_| vec![],
        ))
        .build()
        .unwrap();

    let cfg = db.config();
    assert!(cfg.tables[0].strict);
}

#[test]
fn config_includes_ignore_patterns() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .ignore(vec!["**/skip/**".to_string(), "**/temp/**".to_string()])
        .build()
        .unwrap();

    let cfg = db.config();
    assert_eq!(
        cfg.ignore,
        vec!["**/skip/**".to_string(), "**/temp/**".to_string()],
    );
}

#[test]
fn config_reflects_persist_and_persist_path() {
    let root = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();
    let persist_path: PathBuf = cache_dir.path().join("custom-cache.db");

    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .persist(true)
        .persist_path(&persist_path)
        .build()
        .unwrap();

    let cfg = db.config();
    assert!(cfg.persist);
    assert_eq!(cfg.persist_path, Some(persist_path));
}

#[test]
fn config_defaults_persist_false_persist_path_none() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .table(noop_table("CREATE TABLE items (name TEXT)", "items/*.json"))
        .build()
        .unwrap();

    let cfg = db.config();
    assert!(!cfg.persist);
    assert_eq!(cfg.persist_path, None);
}
