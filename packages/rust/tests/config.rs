//! Integration test for `load_config` reading a real `.dirsql.toml` off disk.
//!
//! The pure TOML-parsing tests (`load_config_str`) stay inline in `config.rs`;
//! this exercises the thin file-reading wrapper, which needs a real file
//! (effectful std), so per the `unit lint` isolation rule it belongs in the
//! integration tier rather than the inline unit module.

use dirsql::config::{ConfigError, load_config, load_config_str};
use tempfile::TempDir;

#[test]
fn load_config_from_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(".dirsql.toml");
    std::fs::write(
        &path,
        r#"
[[table]]
name = "t"
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.csv"
on-file = "cat {path}"
"#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.tables.len(), 1);
}

// Unknown keys are a hard error at every schema level (top level, `[dirsql]`,
// `[[table]]`, `[[dirsql.extension]]`), so a typo or a removed key fails loudly
// instead of silently no-opping.
#[test]
fn unknown_key_at_each_schema_level_errors() {
    let cases = [
        ("top-level", "glbo = \"typo\"\n", "glbo"),
        (
            "[dirsql]",
            "[dirsql]\npersistpath = \"cache.db\"\n",
            "persistpath",
        ),
        (
            "[[table]]",
            "[[table]]\nname = \"t\"\nddl = \"CREATE TABLE t (path TEXT)\"\nglob = \"*.json\"\non-file = \"cat {path}\"\nformat = \"json\"\n",
            "format",
        ),
        (
            "[[dirsql.extension]]",
            "[[dirsql.extension]]\npath = \"vec0.so\"\nentrypont = \"x\"\n",
            "entrypont",
        ),
    ];
    for (level, toml, key) in cases {
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::Toml(_)),
            "{level}: expected a TOML parse error, got: {err:?}"
        );
        assert!(
            err.to_string().contains(key),
            "{level}: error must name the unknown key `{key}`, got: {err}"
        );
    }
}

// The `pre-query` / `post-query` hooks are removed (#803): the keys are no
// longer part of the schema, so a config carrying one fails with the standard
// unknown-key error naming the key -- never silently accepted.
#[test]
fn removed_query_hook_keys_are_rejected_as_unknown() {
    let cases = [
        (
            "pre-query",
            "[dirsql]\npre-query = \"echo SELECT 1 AS one\"\n",
        ),
        ("post-query", "[dirsql]\npost-query = \"cat\"\n"),
    ];
    for (key, toml) in cases {
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::Toml(_)),
            "`{key}`: expected a TOML parse error, got: {err:?}"
        );
        assert!(
            err.to_string().contains(key),
            "`{key}`: error must name the removed key, got: {err}"
        );
    }
}

// `DEFAULT_CONFIG_TOML` (packages/rust/src/cli/mod.rs) crosses the
// cli <-> config module boundary, so this lives here rather than as a unit
// test in either module -- per the `unit lint` isolation rule. Gated on the
// `cli` feature (like the sibling CLI test files) so the default-feature
// `cargo test --workspace` still compiles; the other tests in this file need
// no CLI and keep running without the feature.
#[cfg(feature = "cli")]
#[test]
fn default_config_toml_is_an_escalation_example_with_a_named_table_and_on_file_hook() {
    use dirsql::cli::DEFAULT_CONFIG_TOML;

    let config = load_config_str(DEFAULT_CONFIG_TOML)
        .expect("DEFAULT_CONFIG_TOML must be valid dirsql config TOML");
    assert_eq!(config.tables.len(), 1);
    let table = &config.tables[0];
    // The scaffold escalates past the zero-config path-table floor: it names a
    // table, scopes it with a glob, and pins a schema.
    assert_eq!(table.glob, "**/*.json");
    assert!(
        table.ddl.starts_with("CREATE TABLE records ("),
        "DEFAULT_CONFIG_TOML's DDL must declare the `records` table, got: {}",
        table.ddl
    );
    // The table carries a real `on-file` hook: rows come from the hook, not from
    // stat-fact injection. This keeps the asset loadable once hook-less
    // `[[table]]` entries become a config error.
    assert!(
        !table.on_file.is_empty(),
        "DEFAULT_CONFIG_TOML's table must carry an `on-file` hook, got an empty command",
    );
}
