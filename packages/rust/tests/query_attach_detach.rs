//! Integration tests: `query()` must reject `ATTACH`/`DETACH`.
//!
//! SQLite classifies `ATTACH` as read-only (`sqlite3_stmt_readonly` returns
//! true), so the read-only gate alone lets it through — yet `ATTACH` creates a
//! file on disk and exposes an arbitrary external database to a subsequent
//! `SELECT ... FROM ext.*`. The authorizer on the query path denies both
//! `ATTACH` and `DETACH` at prepare time, so neither ever executes.

use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn items_db(root: &Path) -> DirSQL {
    fs::write(root.join("a.txt"), "apple").unwrap();
    DirSQL::new(
        root,
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap()
}

#[test]
fn query_rejects_attach_and_creates_no_file() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let target = root.path().join("attached.db");
    let sql = format!("ATTACH '{}' AS ext", target.display());
    let err = db.query(&sql).unwrap_err();

    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("not authorized"),
        "ATTACH should be rejected as not authorized, got: {err}"
    );
    assert!(
        !target.exists(),
        "a rejected ATTACH must not create the target database file"
    );
}

#[test]
fn query_rejects_detach() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());

    let err = db.query("DETACH ext").unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("not authorized"),
        "DETACH should be rejected as not authorized, got: {err}"
    );
}

#[test]
fn query_cannot_read_preexisting_external_db_via_attach() {
    // A pre-seeded external SQLite file with a secret table must stay
    // unreadable: the ATTACH that would expose it is denied, so the follow-up
    // `SELECT ... FROM ext.*` never has a schema to read.
    let root = TempDir::new().unwrap();
    let secret = TempDir::new().unwrap();
    let secret_path = secret.path().join("secret.db");
    let external = rusqlite::Connection::open(&secret_path).unwrap();
    external
        .execute_batch("CREATE TABLE secrets (v TEXT); INSERT INTO secrets (v) VALUES ('token');")
        .unwrap();
    drop(external);

    let db = items_db(root.path());
    let attach = format!("ATTACH '{}' AS ext", secret_path.display());
    assert!(
        db.query(&attach).is_err(),
        "ATTACH of a pre-existing external db must be rejected"
    );
    assert!(
        db.query("SELECT v FROM ext.secrets").is_err(),
        "the external db's contents must remain unreadable"
    );
}

#[test]
fn query_still_allows_normal_select() {
    let root = TempDir::new().unwrap();
    let db = items_db(root.path());
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
}
