//! Tests for code-review findings; each test name carries its finding ID.

use dirsql::{DirSQL, DirSqlError, Row, Table, Value};
use std::error::Error as _;
use std::time::Duration;
use tempfile::TempDir;

/// A DDL whose table-name slot contains SQL syntax characters must be
/// rejected at registration time.
#[test]
fn s1_ddl_with_semicolon_in_table_name_slot_is_rejected() {
    let dir = TempDir::new().unwrap();
    let result = DirSQL::new(
        dir.path(),
        vec![Table::new(
            "CREATE TABLE evil;DROP_TABLE_bar--(id TEXT)",
            "*.json",
            |_| vec![],
        )],
    );
    // `DirSQL` doesn't implement `Debug`, so we can't use `expect_err`.
    let err = match result {
        Ok(_) => panic!("DDL with `;` in the table-name slot must be rejected"),
        Err(e) => e,
    };
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("identifier") || msg.contains("invalid table"),
        "expected an identifier-validation message, got: {msg}"
    );
}

/// A column name returned by `extract` that contains SQL syntax must produce
/// a clean validation error. Strict mode is required: relaxed mode silently
/// drops unknown keys, so the validator could never fire.
#[test]
fn s1_column_name_with_sql_syntax_produces_clean_error() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.json"), b"{}").unwrap();
    let result = DirSQL::new(
        dir.path(),
        vec![Table::strict("CREATE TABLE t (id TEXT)", "*.json", |_| {
            let mut row = Row::new();
            row.insert("id); DROP TABLE t; --".into(), Value::Text("x".into()));
            vec![row]
        })],
    );
    let err = match result {
        Ok(_) => panic!("column name with SQL syntax must be rejected"),
        Err(e) => e,
    };
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("identifier")
            || msg.contains("invalid column")
            || msg.contains("invalid identifier"),
        "expected an identifier-validation message, got: {msg}"
    );
}

fn one_row_db(dir: &TempDir) -> DirSQL {
    std::fs::write(dir.path().join("a.json"), b"{}").unwrap();
    DirSQL::new(
        dir.path(),
        vec![Table::new("CREATE TABLE t (id TEXT)", "*.json", |_| {
            let mut row = Row::new();
            row.insert("id".into(), Value::Text("x".into()));
            vec![row]
        })],
    )
    .unwrap()
}

#[test]
fn s2_dirsql_filter_not_bypassed_by_comment_mention() {
    let dir = TempDir::new().unwrap();
    let db = one_row_db(&dir);

    let rows = db.query("SELECT * FROM t /* _dirsql_file_path */").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        !rows[0].contains_key("_dirsql_file_path"),
        "_dirsql_file_path mentioned only in a comment must not be exposed: {:?}",
        rows[0],
    );
}

#[test]
fn s2_dirsql_filter_not_bypassed_by_string_literal() {
    let dir = TempDir::new().unwrap();
    let db = one_row_db(&dir);

    let rows = db
        .query("SELECT * FROM t WHERE id != '_dirsql_file_path'")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        !rows[0].contains_key("_dirsql_file_path"),
        "_dirsql_file_path inside a string literal must not be exposed: {:?}",
        rows[0],
    );
}

// Peak-memory behavior isn't testable in CI; pin instead that hashing stays
// correct for a large payload across any streaming refactor.
#[test]
fn p5_hash_file_matches_blake3_of_2mib_payload() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.bin");
    let bytes = vec![0xABu8; 2 * 1024 * 1024];
    std::fs::write(&path, &bytes).unwrap();

    let h = dirsql::persist::hash_file(&path).unwrap();
    let expected = *blake3::hash(&bytes).as_bytes();
    assert_eq!(h, expected);
}

#[test]
fn p7_builder_exposes_poll_interval() {
    let dir = TempDir::new().unwrap();
    let _db = DirSQL::builder()
        .root(dir.path())
        .table(Table::new("CREATE TABLE t (id TEXT)", "*.json", |_| vec![]))
        .poll_interval(Duration::from_millis(50))
        .build()
        .unwrap();
}

#[test]
fn i3_watch_error_exposes_underlying_source() {
    let dir = TempDir::new().unwrap();
    let db = DirSQL::new(
        dir.path(),
        vec![Table::new("CREATE TABLE t (id TEXT)", "*.json", |_| vec![])],
    )
    .unwrap();
    // Delete the directory out from under the watcher so `start_watching`
    // fails attaching `notify` to a missing path.
    drop(dir);
    let err = db
        .start_watching()
        .expect_err("start_watching against vanished root must fail");
    match &err {
        DirSqlError::Watch { .. } => {}
        other => panic!("expected DirSqlError::Watch, got: {other:?}"),
    }
    assert!(
        err.source().is_some(),
        "DirSqlError::Watch must expose its underlying source (got None)"
    );
}

#[test]
fn i6_parser_versions_json_no_longer_lists_removed_parsers() {
    let s = dirsql::persist::PARSER_VERSIONS_JSON;
    for removed in ["csv", "tsv", "yaml", "md", "toml", "json", "jsonl"] {
        assert!(
            !s.contains(&format!("\"{removed}\"")),
            "PARSER_VERSIONS_JSON still names removed parser `{removed}`: {s}"
        );
    }
}

#[test]
fn i8_ext_preserves_original_case() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("Photo.JPG"), b"").unwrap();
    let db = DirSQL::new(
        dir.path(),
        vec![Table::new("CREATE TABLE pics (ext TEXT)", "**/*", |_| {
            vec![Row::new()]
        })],
    )
    .unwrap();
    let rows = db.query("SELECT ext FROM pics").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["ext"],
        Value::Text("JPG".into()),
        "ext must preserve the original case, got: {:?}",
        rows[0]["ext"],
    );
}

#[cfg(feature = "cli")]
#[test]
fn i11_app_state_has_from_string_for_unavailable() {
    use dirsql::cli::AppState;
    let state: AppState = String::from("config failed to load").into();
    match state {
        AppState::Unavailable(msg) => assert_eq!(msg, "config failed to load"),
        AppState::Ready(_) => panic!("From<String> must produce AppState::Unavailable"),
    }
}
