//! Integration coverage for loading SQLite extensions via config (#225).
//!
//! These exercise the public construction surface (`DirSQL::from_config`) — a
//! `.dirsql.toml` that declares `[[dirsql.extension]]` entries. dirsql loads
//! each configured extension onto the connection at startup, before any
//! `CREATE TABLE`, then disables loading again so the SQL `load_extension()`
//! function is never left open.

use dirsql::DirSQL;
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
