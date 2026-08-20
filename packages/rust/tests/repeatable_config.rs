//! Integration red tests for #545: `DirSQLBuilder::config()` is repeatable.
//!
//! Each call appends to the ordered config list #553 resolves; call order is
//! accumulation order; one call stays byte-identical to today. Today the
//! second call replaces the first, so the multi-call expectations fail on
//! their assertions.

use std::fs;
use std::path::Path;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// Write `.dirsql.toml` with `contents` into `dir` and return its path.
fn write_config(dir: &Path, contents: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(&path, contents).unwrap();
    path
}

fn table_config(name: &str) -> String {
    format!(
        r#"
[[table]]
name = "{name}"
ddl = "CREATE TABLE {name} (basename TEXT)"
glob = "*.json"
on-file = '''sh -c 'printf "[{{\"basename\":\"%s\"}}]" "${{1##*/}}"' sh {{path}}'''
"#
    )
}

#[test]
fn config_accumulates_across_calls() {
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();

    let cfg_a = TempDir::new().unwrap();
    let cfg_a_path = write_config(cfg_a.path(), &table_config("alpha"));
    let cfg_b = TempDir::new().unwrap();
    let cfg_b_path = write_config(cfg_b.path(), &table_config("beta"));
    let cfg_c = TempDir::new().unwrap();
    let cfg_c_path = write_config(cfg_c.path(), &table_config("gamma"));

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_a_path)
        .config(&cfg_b_path)
        .config(&cfg_c_path)
        .build()
        .expect("every .config() call must load");

    for table in ["alpha", "beta", "gamma"] {
        let rows = db
            .query(&format!("SELECT basename FROM {table}"))
            .unwrap_or_else(|err| {
                panic!("table {table} from an accumulated config must be queryable: {err}")
            });
        assert_eq!(rows.len(), 1, "table {table} must index the data dir");
        assert_eq!(rows[0]["basename"], Value::Text("a.json".into()));
    }
}

#[test]
fn a_single_config_call_is_unchanged() {
    let data = TempDir::new().unwrap();
    fs::write(data.path().join("a.json"), "{}").unwrap();

    let cfg = TempDir::new().unwrap();
    let cfg_path = write_config(cfg.path(), &table_config("alpha"));

    let db = DirSQL::builder()
        .root(data.path())
        .config(&cfg_path)
        .build()
        .expect("a single .config() call must behave exactly as before");

    let rows = db.query("SELECT basename FROM alpha").unwrap();
    assert_eq!(rows.len(), 1);
}
