//! Integration test for `load_config` reading a real `.dirsql.toml` off disk.
//!
//! The pure TOML-parsing tests (`load_config_str`) stay inline in `config.rs`;
//! this exercises the thin file-reading wrapper, which needs a real file
//! (effectful std), so per the `unit lint` isolation rule it belongs in the
//! integration tier rather than the inline unit module.

use dirsql::config::load_config;
use tempfile::TempDir;

#[test]
fn load_config_from_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(".dirsql.toml");
    std::fs::write(
        &path,
        r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.tables.len(), 1);
}
