//! Integration test for `load_config` reading a real `.dirsql.toml` off disk.
//!
//! The pure TOML-parsing tests (`load_config_str`) stay inline in `config.rs`;
//! this exercises the thin file-reading wrapper, which needs a real file
//! (effectful std), so per the `unit lint` isolation rule it belongs in the
//! integration tier rather than the inline unit module.

use dirsql::cli::DEFAULT_CONFIG_TOML;
use dirsql::config::{load_config, load_config_str};
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

// `DEFAULT_CONFIG_TOML` (packages/rust/src/cli/mod.rs) crosses the
// cli <-> config module boundary, so this lives here rather than as a unit
// test in either module -- per the `unit lint` isolation rule.
#[test]
fn default_config_toml_parses_to_a_single_files_table_with_every_stat_column() {
    let config = load_config_str(DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must be valid dirsql config TOML");
    assert_eq!(config.tables.len(), 1);
    let table = &config.tables[0];
    assert_eq!(table.glob, "**/*");
    assert!(table.ddl.starts_with("CREATE TABLE files ("));
    for col in ["path", "basename", "dir", "ext", "size", "mtime", "ctime"] {
        assert!(
            table.ddl.contains(col),
            "DEFAULT_CONFIG_TOML's DDL must declare {col}, got: {}",
            table.ddl
        );
    }
}
