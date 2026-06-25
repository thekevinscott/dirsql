//! Integration coverage for loading SQLite extensions via config (#225).
//!
//! These exercise the public construction surface (`DirSQL::from_config`) — a
//! `.dirsql.toml` that declares `[[dirsql.extension]]` entries. dirsql loads
//! each configured extension onto the connection at startup, before any
//! `CREATE TABLE`, then disables loading again so the SQL `load_extension()`
//! function is never left open.

use dirsql::{DirSQL, Extension};
use std::fs;
use tempfile::TempDir;

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
ddl = "CREATE TABLE files (_path TEXT)"
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
/// parent directory. The file is absent so construction fails — exercising the
/// relative-path resolution branch end to end.
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
