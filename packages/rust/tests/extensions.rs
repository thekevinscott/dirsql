//! Integration coverage for loading SQLite extensions via config.
//!
//! These exercise the public construction surface (`DirSQL::from_config`) — a
//! `.dirsql.toml` that declares `[[dirsql.extension]]` entries. dirsql loads
//! each configured extension onto the connection at startup, before any
//! `CREATE TABLE`, then disables loading again so the SQL `load_extension()`
//! function is never left open.

use dirsql::{DirSQL, Extension, Value};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Build the test-only loadable extension fixture
/// (`tests/fixtures/testext`) and return the path to its compiled shared
/// library. Built into `CARGO_TARGET_TMPDIR` with coverage/instrumentation
/// env removed so the nested build is independent of an outer
/// `cargo llvm-cov` invocation.
fn build_fixture_extension() -> PathBuf {
    let manifest = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/testext/Cargo.toml"
    );
    let output = Command::new(env!("CARGO"))
        .args([
            "build",
            "--manifest-path",
            manifest,
            "--target-dir",
            env!("CARGO_TARGET_TMPDIR"),
            "--message-format=json",
        ])
        .env_remove("RUSTFLAGS")
        .env_remove("RUSTDOCFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("failed to spawn cargo build for the extension fixture");
    assert!(
        output.status.success(),
        "fixture build failed:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    // Parse cargo's JSON output for the cdylib artifact path (platform-
    // independent: .so / .dylib / .dll).
    let stdout = String::from_utf8(output.stdout).unwrap();
    let mut artifact: Option<PathBuf> = None;
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"] == "compiler-artifact" {
            if let Some(files) = msg["filenames"].as_array() {
                for f in files.iter().filter_map(|f| f.as_str()) {
                    if f.ends_with(".so") || f.ends_with(".dylib") || f.ends_with(".dll") {
                        artifact = Some(PathBuf::from(f));
                    }
                }
            }
        }
    }
    artifact.expect("no cdylib artifact in cargo build output for the fixture")
}

/// A config that names an extension file which does not exist on disk must
/// fail construction. dirsql loads configured extensions at startup and
/// surfaces load failures rather than silently ignoring them.
#[test]
fn missing_extension_path_fails_construction() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.extension]]
path = "/nonexistent/dirsql-no-such-extension.so"

[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.txt"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x").unwrap();

    let result = DirSQL::from_config(root.path());
    assert!(
        result.is_err(),
        "expected construction to fail when a configured extension file is missing, got Ok",
    );
}

/// The programmatic `DirSQLBuilder::extension` surface loads at startup too:
/// a missing file fails the build.
#[test]
fn builder_extension_method_surfaces_missing_file() {
    let root = TempDir::new().unwrap();
    let result = DirSQL::builder()
        .root(root.path())
        .extension(Extension {
            path: "/nonexistent/dirsql-x.so".into(),
            entrypoint: None,
        })
        .build();
    assert!(
        result.is_err(),
        "missing extension via .extension() should fail the build",
    );
}

/// `DirSQLBuilder::extensions` (plural, replacing the list) carries the same
/// load-at-startup behavior, including a custom entrypoint.
#[test]
fn builder_extensions_method_surfaces_missing_file() {
    let root = TempDir::new().unwrap();
    let result = DirSQL::builder()
        .root(root.path())
        .extensions(vec![Extension {
            path: "/nonexistent/dirsql-y.so".into(),
            entrypoint: Some("sqlite3_y_init".into()),
        }])
        .build();
    assert!(result.is_err());
}

/// A relative extension path in a config file resolves against the config's
/// parent directory. The file is absent, so construction fails.
#[test]
fn config_relative_extension_path_is_resolved() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.extension]]
path = "ext/local-extension.so"
"#,
    )
    .unwrap();

    let result = DirSQL::from_config(root.path());
    assert!(result.is_err());
}

/// End to end: a real extension declared in config is loaded onto the
/// connection at startup, so a query can call the function it registered. Also
/// asserts the security outcome — `load_extension()` issued through the public
/// query surface is rejected after startup.
#[test]
fn loads_real_extension_and_calls_registered_function() {
    let ext = build_fixture_extension();

    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            r#"
[[dirsql.extension]]
path = "{}"
entrypoint = "sqlite3_extension_init"

[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.txt"
"#,
            ext.display(),
        ),
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x").unwrap();

    let db = DirSQL::from_config(root.path())
        .expect("construction with a real extension should succeed");

    // The fixture registered dirsql_testext_answer() -> 42.
    let rows = db.query("SELECT dirsql_testext_answer() AS a").unwrap();
    assert_eq!(rows[0]["a"], Value::Integer(42));

    // Loading was disabled again after startup, so a load_extension() call
    // through the public query surface is rejected. NB: the read-only query
    // guard is NOT what blocks it — SQLite classifies `SELECT load_extension()`
    // as read-only, so it sails past that guard — the sole protection is that
    // extension loading itself is disabled after startup.
    assert!(
        db.query(&format!("SELECT load_extension('{}')", ext.display()))
            .is_err(),
        "load_extension() via query() must be rejected after startup",
    );
}

/// `suppress_config_extensions(true)` makes the builder ignore a config file's
/// own `[[dirsql.extension]]` entries and use only the programmatically-supplied
/// ones — the seam the CLI launcher uses to hand the core already-resolved
/// (e.g. package-name → path) extensions. The config's bogus relative path
/// would fail to load if honored; the build succeeds and the overriding
/// extension's function is callable.
#[test]
fn suppress_config_extensions_ignores_config_entries_and_uses_overrides() {
    let ext = build_fixture_extension();

    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.extension]]
path = "does/not/exist.so"

[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.txt"
"#,
    )
    .unwrap();
    fs::write(root.path().join("a.txt"), "x").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .suppress_config_extensions(true)
        .extensions(vec![Extension {
            path: ext,
            entrypoint: Some("sqlite3_extension_init".into()),
        }])
        .build()
        .expect("build should ignore the config's bogus extension and load the override");

    let rows = db.query("SELECT dirsql_testext_answer() AS a").unwrap();
    assert_eq!(rows[0]["a"], Value::Integer(42));
}

/// Guard: without `suppress_config_extensions`, the config's bogus extension is
/// honored and the build fails — proving the suppression above is what changed
/// the outcome.
#[test]
fn config_extensions_are_loaded_by_default() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.extension]]
path = "does/not/exist.so"
"#,
    )
    .unwrap();

    let result = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build();
    assert!(
        result.is_err(),
        "a config's extension entry must load by default (no suppression)",
    );
}

/// A failed extension load surfaces an error naming the offending library,
/// not an opaque generic SQLite error.
#[test]
fn missing_extension_error_names_the_extension() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[dirsql.extension]]
path = "/nonexistent/dirsql-no-such-extension.so"
"#,
    )
    .unwrap();

    // `DirSQL` is not `Debug`, so match rather than `unwrap_err()`.
    let err = match DirSQL::from_config(root.path()) {
        Ok(_) => panic!("expected construction to fail for a missing extension"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("failed to load extension"),
        "error should name the failed extension, got: {err}"
    );
}
