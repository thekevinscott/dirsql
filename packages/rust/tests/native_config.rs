//! Integration tests for native-language config support (`dirsql::cli::native_config`).
//!
//! These drive `InterpretHelper::from_child` against a **real** subprocess
//! (a `bash`/`true` child whose stdin/stdout are piped) and exercise
//! `build_dirsql` end-to-end over real fixture files written to a temp
//! directory. That makes them integration tests, not unit tests: the
//! handshake/extract NDJSON protocol is verified through the public
//! `dirsql::cli::native_config` surface against a live process, with real
//! `std::process::Command` spawning and real `std::fs::write` fixtures.
//!
//! They were moved here out of `native_config.rs`'s inline `#[cfg(test)]`
//! module so that module stays purely unit -- the `testing-conventions`
//! `unit lint` isolation rule forbids effectful std (`std::process`,
//! `std::fs`) in unit tests. The pure wire-format tests
//! (`parse_handshake` / `dispatch_extract` over in-memory `Cursor`/`Vec`
//! streams) remain inline next to the private functions they exercise.
//!
//! Gated behind `--features cli` -- the module under test lives in
//! `src/cli/`, which is only compiled when that feature is on. Compiled to
//! an empty test binary otherwise so `cargo test` (no features) still
//! succeeds.

#![cfg(feature = "cli")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use dirsql::Value;
use dirsql::cli::native_config::{InterpretHelper, NativeConfig, build_dirsql};
use tempfile::TempDir;

/// Spawn a fake helper that prints `handshake` once on stdout then emits
/// one canned response per line received on stdin (an infinite loop until
/// stdin closes). Returns the helper paired with the parsed
/// [`NativeConfig`]. Drives the production `InterpretHelper::from_child`
/// constructor so the post-spawn plumbing is exercised by these tests.
fn spawn_fake_helper(
    handshake: &str,
    response_per_request: &str,
) -> (Arc<InterpretHelper>, NativeConfig) {
    let child = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "printf '%s\\n' '{handshake}'; while IFS= read -r line; do printf '%s\\n' '{response_per_request}'; done",
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    InterpretHelper::from_child(child).unwrap()
}

#[test]
fn build_dirsql_threads_persist_and_persist_path_into_the_builder() {
    // Cover the `if config.persist` and `if let Some(p) = config.persist_path`
    // branches in `build_dirsql`. A real persist build needs the cache file
    // to live under root, so point persist_path at a path inside the
    // tempdir and pass persist=true.
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.json"), b"{}").unwrap();
    let cache_path = tmp.path().join(".dirsql/cache.db");

    let handshake = format!(
        r#"{{"type":"config","state":{{"root":"{}","tables":[{{"ddl":"CREATE TABLE papers (title TEXT)","glob":"*.json"}}],"persist":true,"persist_path":"{}"}}}}"#,
        tmp.path().display(),
        cache_path.display(),
    );
    let (helper, config) = spawn_fake_helper(
        &handshake,
        r#"{"type":"result","id":1,"ok":true,"rows":[{"title":"y"}]}"#,
    );
    assert!(config.persist);
    assert_eq!(config.persist_path.as_ref().unwrap(), &cache_path);

    let db = build_dirsql(helper, config).unwrap();
    let rows = db.query("SELECT COUNT(*) AS n FROM papers").unwrap();
    assert_eq!(rows[0].get("n"), Some(&Value::Integer(1)));
    assert!(cache_path.exists(), "persist build should create the cache");
}

#[test]
fn build_dirsql_threads_extensions_into_the_builder() {
    // A handshake whose `extensions` names a missing shared library must
    // fail the build, proving the parsed extensions reach the core's
    // load-at-startup path (enable -> load -> disable). (#229)
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.json"), b"{}").unwrap();

    let handshake = format!(
        r#"{{"type":"config","state":{{"root":"{}","tables":[{{"ddl":"CREATE TABLE papers (title TEXT)","glob":"*.json"}}],"extensions":[{{"path":"/nonexistent/dirsql-no-such-ext.so"}}]}}}}"#,
        tmp.path().display(),
    );
    let (helper, config) = spawn_fake_helper(
        &handshake,
        r#"{"type":"result","id":1,"ok":true,"rows":[{"title":"y"}]}"#,
    );
    assert_eq!(config.extensions.len(), 1);
    assert_eq!(
        config.extensions[0].path,
        PathBuf::from("/nonexistent/dirsql-no-such-ext.so"),
    );

    let err = match build_dirsql(helper, config) {
        Ok(_) => panic!("expected build to fail on a missing extension"),
        Err(e) => e,
    };
    assert!(err.contains("failed to load extension"), "got: {err}");
}

#[test]
fn build_dirsql_round_trip_with_fake_helper_invokes_extract_per_matched_file() {
    // Create a tempdir with two matching files so the scan invokes
    // `extract` twice — exercising the closure created inside
    // `build_dirsql`, the `dispatch_extract` path through the
    // helper's IO mutex, and end-to-end DDL/glob wiring.
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.json"), b"{}").unwrap();
    std::fs::write(tmp.path().join("b.json"), b"{}").unwrap();

    let handshake = format!(
        r#"{{"type":"config","state":{{"root":"{}","tables":[{{"ddl":"CREATE TABLE papers (title TEXT)","glob":"*.json"}}]}}}}"#,
        tmp.path().display(),
    );
    let (helper, config) = spawn_fake_helper(
        &handshake,
        r#"{"type":"result","id":1,"ok":true,"rows":[{"title":"x"}]}"#,
    );

    let db = build_dirsql(helper, config).unwrap();
    let rows = db.query("SELECT COUNT(*) AS n FROM papers").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("n"), Some(&Value::Integer(2)));
}

#[test]
fn helper_extract_round_trip_surfaces_extracted_columns_against_a_fake_subprocess() {
    // Drive the helper's private `extract` through the public `build_dirsql`
    // surface: two matching files make the scan dispatch two extract
    // requests (incrementing the id counter), and the canned `title` value
    // each helper reply carries must land in the queried rows. This
    // exercises the same IO-mutex / `dispatch_extract` path the old inline
    // `helper.extract(...)` test hit, but without reaching the private
    // method (integration can only see the public API).
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("a.json"), b"{}").unwrap();
    std::fs::write(tmp.path().join("b.json"), b"{}").unwrap();

    let handshake = format!(
        r#"{{"type":"config","state":{{"root":"{}","tables":[{{"ddl":"CREATE TABLE papers (title TEXT)","glob":"*.json"}}]}}}}"#,
        tmp.path().display(),
    );
    let (helper, config) = spawn_fake_helper(
        &handshake,
        r#"{"type":"result","id":1,"ok":true,"rows":[{"title":"Alpha"}]}"#,
    );

    let db = build_dirsql(helper, config).unwrap();
    let rows = db.query("SELECT title FROM papers GROUP BY title").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("title"), Some(&Value::Text("Alpha".into())));
}

#[test]
fn helper_from_child_errors_when_stdin_was_not_piped() {
    // `Command::spawn` without `.stdin(Stdio::piped())` inherits the
    // parent's stdin — `child.stdin.take()` returns `None`, driving
    // the first `.ok_or_else` arm in `from_child`.
    let child = Command::new("true").spawn().unwrap();
    let err = match InterpretHelper::from_child(child) {
        Ok(_) => panic!("expected error when stdin is not piped"),
        Err(e) => e,
    };
    assert!(
        err.contains("failed to capture interpret stdin"),
        "got: {err}",
    );
}

#[test]
fn helper_from_child_errors_when_stdout_was_not_piped() {
    // Pipe stdin but leave stdout inherited — `child.stdout.take()`
    // returns `None`, driving the second `.ok_or_else` arm.
    let child = Command::new("true").stdin(Stdio::piped()).spawn().unwrap();
    let err = match InterpretHelper::from_child(child) {
        Ok(_) => panic!("expected error when stdout is not piped"),
        Err(e) => e,
    };
    assert!(
        err.contains("failed to capture interpret stdout"),
        "got: {err}",
    );
}

#[test]
fn helper_from_child_errors_when_subprocess_emits_no_handshake() {
    // A subprocess that exits without writing anything drives
    // `from_child` through to `parse_handshake`'s empty-line arm.
    let child = Command::new("true")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let err = match InterpretHelper::from_child(child) {
        Ok(_) => panic!("expected error from empty-stdout subprocess"),
        Err(e) => e,
    };
    assert!(
        err.contains("exited before sending handshake"),
        "got: {err}"
    );
}

#[test]
fn build_dirsql_errors_when_ddl_has_no_table_name() {
    // The DDL is rejected by `parse_table_name` before any extract closure
    // is created. The handshake carries an unparseable DDL through to the
    // resolved `NativeConfig` (the parser stores the DDL verbatim), so
    // `build_dirsql` fails on it. A live (but otherwise unused) helper
    // satisfies the `build_dirsql` signature.
    let (helper, config) = spawn_fake_helper(
        r#"{"type":"config","state":{"root":"/tmp","tables":[{"ddl":"NOT VALID DDL","glob":"*.json"}]}}"#,
        r#"{"type":"result","id":1,"ok":true}"#,
    );
    // `DirSQL` doesn't impl Debug, so we can't use `unwrap_err` directly.
    let err = match build_dirsql(helper, config) {
        Ok(_) => panic!("expected build_dirsql to fail on invalid DDL"),
        Err(e) => e,
    };
    assert!(err.contains("could not parse table name"), "got: {err}");
}
