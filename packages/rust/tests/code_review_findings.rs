//! Red tests for code-review findings (issue #218).
//!
//! Each test corresponds to a numbered finding from the review. They assert
//! the *target* behavior — they fail today and turn green as each finding is
//! addressed. Tests are grouped by finding ID so the mapping to #218 stays
//! easy to navigate as fixes land.

use dirsql::{DirSQL, DirSqlError, Row, Table, Value};
use std::error::Error as _;
use std::time::Duration;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// S1 — SQL injection surface via unparameterized table/column identifiers
//
// Goal: identifier validation at registration / insert time, producing a
// clean error rather than relying on rusqlite's "execute only runs the first
// statement" accident.
// ---------------------------------------------------------------------------

/// A DDL whose table name slot contains SQL syntax characters must be
/// rejected at registration time. Today `parse_table_name` happily returns
/// the poisoned name and the malicious DDL is partially executed.
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
/// a clean validation error, not a cryptic SQLite parse failure.
///
/// In strict mode the bad key reaches the normalize step (relaxed mode would
/// silently drop unknown keys — by design — so the validator can't fire
/// there). Strict mode is the appropriate vehicle for asserting the
/// identifier check.
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

// ---------------------------------------------------------------------------
// S2 — `_dirsql_*` filter is a substring check, not a real boundary
//
// Goal: the filter inspects the actual SQL projection, not the raw text. A
// comment or string literal that happens to contain `_dirsql_file_path` must
// not expose the tracking column.
// ---------------------------------------------------------------------------

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

/// `SELECT *` must not expose `_dirsql_*` columns just because a SQL comment
/// happens to mention the name. Today `sql.contains(name)` matches the
/// comment text and leaks the column.
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

/// `SELECT *` must not expose `_dirsql_*` columns when the name only appears
/// inside a SQL string literal.
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

// ---------------------------------------------------------------------------
// P4 — `compute_stat_virtuals` re-stats the file
//
// Goal: a single `metadata()` call per upsert. The current code stats once
// in `handle_upsert` and again inside `compute_stat_virtuals`. We can't
// directly count syscalls portably, but we can pin the *result* by asserting
// `_size` reflects the metadata the handler saw — not a fresh stat that
// might race with concurrent modification. Today this isn't observable from
// outside; we leave a placeholder that the refactor will activate.
// ---------------------------------------------------------------------------

// (no red test — purely internal; covered by the structural change.)

// ---------------------------------------------------------------------------
// P5 — `hash_file` slurps entire file
//
// Goal: hashing uses an incremental reader so peak memory stays bounded.
// Can't easily test memory in CI; we instead pin that the hash function
// remains correct after the streaming refactor by hashing a 2 MiB file and
// comparing against the known BLAKE3 digest. This test passes today; it
// guards against a regression once the implementation changes.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// P7 — `run_channel_loop` hard-codes 200ms poll latency
//
// Goal: the builder exposes a `poll_interval` knob so consumers can choose
// between latency and idle CPU. Today the constant is private and there is
// no setter.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// I3 — Stringly-typed error variants lose information
//
// Goal: `DirSqlError::Watch`, `Matcher`, `Config` retain their underlying
// source via `Error::source()` so callers can downcast.
// ---------------------------------------------------------------------------

#[test]
fn i3_watch_error_exposes_underlying_source() {
    // Construct a Watch error via the public `watch()` path on a DB whose
    // watcher was poisoned (or otherwise broken). The simplest reproducible
    // failure: call `start_watching` against a path that can't be watched
    // (a nonexistent root). Then assert the resulting DirSqlError::Watch
    // has a non-None `source()`.
    let dir = TempDir::new().unwrap();
    let db = DirSQL::new(
        dir.path(),
        vec![Table::new("CREATE TABLE t (id TEXT)", "*.json", |_| vec![])],
    )
    .unwrap();
    // Delete the directory out from under the watcher. start_watching now
    // tries to attach `notify` to a missing path and fails.
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

// ---------------------------------------------------------------------------
// I6 — `PARSER_VERSIONS_JSON` is dead metadata
//
// Goal: the constant is deleted (or set to `{}`) since the per-format
// parsers it tracks were removed in #169. Today it lists `json, jsonl, csv,
// tsv, toml, yaml, md` — none of which exist as built-in parsers anymore.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// I8 — `_ext` is lowercased
//
// Goal: preserve case so case-sensitive filesystems can distinguish
// `Photo.JPG` from `photo.jpg`.
// ---------------------------------------------------------------------------

#[test]
fn i8_ext_preserves_original_case() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("Photo.JPG"), b"").unwrap();
    let db = DirSQL::new(
        dir.path(),
        vec![Table::new("CREATE TABLE pics (_ext TEXT)", "**/*", |_| {
            vec![Row::new()]
        })],
    )
    .unwrap();
    let rows = db.query("SELECT _ext FROM pics").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["_ext"],
        Value::Text("JPG".into()),
        "_ext must preserve the original case, got: {:?}",
        rows[0]["_ext"],
    );
}

// ---------------------------------------------------------------------------
// I11 — `AppState` only has `From<DirSQL>`
//
// Goal: symmetric `From<String>` for the `Unavailable` arm so call sites
// can stop hand-rolling `AppState::Unavailable(format!(...))`.
// ---------------------------------------------------------------------------

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
