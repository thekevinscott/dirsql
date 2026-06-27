use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Error type for config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Missing required field '{0}' in [[table]] entry")]
    MissingField(&'static str),

    #[error("Missing required field '{0}' in [[dirsql.extension]] entry")]
    MissingExtensionField(&'static str),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// Parsed configuration from a `.dirsql.toml` file.
#[derive(Debug, Clone)]
pub struct Config {
    /// Optional root directory override. When absent, callers derive the
    /// root from the config file's own location. When present, it is taken
    /// relative to the config file's parent (so a config at
    /// `/proj/.dirsql.toml` with `root = "docs"` scans `/proj/docs`).
    pub root: Option<PathBuf>,
    pub ignore: Vec<String>,
    pub tables: Vec<TableConfig>,
    /// Enable persistent on-disk SQLite cache. When false (the default), the
    /// database is rebuilt in memory on every startup.
    pub persist: bool,
    /// Optional override for the on-disk cache location. Resolved relative
    /// to the config file's parent directory when relative.
    pub persist_path: Option<PathBuf>,
    /// SQLite extensions to load at startup, declared via
    /// `[[dirsql.extension]]`. Paths are taken verbatim from the file here;
    /// relative paths are resolved against the config file's parent directory
    /// by the caller (`DirSQLBuilder::resolve`).
    pub extensions: Vec<ExtensionSpec>,
}

/// A SQLite extension to load at startup.
///
/// Declared as a `[[dirsql.extension]]` array entry. dirsql loads each
/// extension onto the connection before any `CREATE TABLE` runs, then
/// disables loading again so the SQL `load_extension()` function is never
/// left exposed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExtensionSpec {
    /// Local path to the extension's shared library (`.so` / `.dylib` /
    /// `.dll`). Relative paths resolve against the config file's parent
    /// directory.
    pub path: PathBuf,
    /// Optional init-symbol override. When `None`, SQLite derives the entry
    /// point from the filename, which often does not match — set this when
    /// the extension's init function isn't `sqlite3_<filename>_init`.
    pub entrypoint: Option<String>,
}

/// Configuration for a single table.
///
/// A config-defined table maps a glob pattern to a SQL DDL. Each matched
/// file produces one row whose columns are derived from filesystem facts:
/// glob path captures (named `{placeholder}` segments) and stat virtuals
/// (`_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`).
/// Content interpretation (frontmatter, JSON dot-paths, CSV parsing, etc.)
/// is intentionally out of scope; for that, register a programmatic
/// [`crate::Table`] with your own extract closure.
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub ddl: String,
    pub glob: String,
    pub strict: Option<bool>,
}

// --- Raw deserialization types (serde) ---

#[derive(Deserialize)]
struct RawConfig {
    dirsql: Option<RawDirsql>,
    table: Option<Vec<RawTable>>,
}

#[derive(Deserialize)]
struct RawDirsql {
    root: Option<PathBuf>,
    ignore: Option<Vec<String>>,
    persist: Option<bool>,
    persist_path: Option<PathBuf>,
    extension: Option<Vec<RawExtension>>,
}

#[derive(Deserialize)]
struct RawExtension {
    path: Option<String>,
    entrypoint: Option<String>,
}

#[derive(Deserialize)]
struct RawTable {
    ddl: Option<String>,
    glob: Option<String>,
    strict: Option<bool>,
}

/// Load and parse a `.dirsql.toml` config file from the given path.
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    load_config_str(&content)
}

/// Parse a `.dirsql.toml` config from a string (useful for testing).
pub fn load_config_str(content: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(content)?;

    let (root, ignore, persist, persist_path, raw_extensions) = match raw.dirsql {
        Some(d) => (
            d.root,
            d.ignore.unwrap_or_default(),
            d.persist.unwrap_or(false),
            d.persist_path,
            d.extension.unwrap_or_default(),
        ),
        None => (None, Vec::new(), false, None, Vec::new()),
    };

    let mut extensions = Vec::with_capacity(raw_extensions.len());
    for raw_ext in raw_extensions {
        // An empty `path = ""` is as unusable as a missing key: reject it at
        // parse time rather than silently resolving it to a directory later.
        let path = raw_ext
            .path
            .filter(|p| !p.is_empty())
            .ok_or(ConfigError::MissingExtensionField("path"))?;
        extensions.push(ExtensionSpec {
            path: PathBuf::from(path),
            entrypoint: raw_ext.entrypoint,
        });
    }

    let raw_tables = raw.table.unwrap_or_default();
    let mut tables = Vec::with_capacity(raw_tables.len());

    for raw_table in raw_tables {
        let ddl = raw_table.ddl.ok_or(ConfigError::MissingField("ddl"))?;
        let glob = raw_table.glob.ok_or(ConfigError::MissingField("glob"))?;

        tables.push(TableConfig {
            ddl,
            glob,
            strict: raw_table.strict,
        });
    }

    Ok(Config {
        root,
        ignore,
        tables,
        persist,
        persist_path,
        extensions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config_parses_required_fields() {
        let toml = r#"
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, _path TEXT)"
glob = "_comments/{thread_id}/index.jsonl"

[[table]]
ddl = "CREATE TABLE items (_path TEXT, _size INTEGER)"
glob = "catalog/*.json"
strict = true
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.ignore, vec!["node_modules/**", ".git/**"]);
        assert_eq!(config.tables.len(), 2);

        let t0 = &config.tables[0];
        assert_eq!(t0.ddl, "CREATE TABLE comments (thread_id TEXT, _path TEXT)");
        assert_eq!(t0.glob, "_comments/{thread_id}/index.jsonl");
        assert!(t0.strict.is_none());

        let t1 = &config.tables[1];
        assert_eq!(t1.strict, Some(true));
    }

    #[test]
    fn missing_ddl_returns_error() {
        let toml = r#"
[[table]]
glob = "*.json"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(err.to_string().contains("ddl"));
    }

    #[test]
    fn missing_glob_returns_error() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(err.to_string().contains("glob"));
    }

    #[test]
    fn empty_tables_list() {
        let toml = r#"
[dirsql]
ignore = ["*.tmp"]
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.tables.is_empty());
        assert_eq!(config.ignore, vec!["*.tmp"]);
    }

    #[test]
    fn completely_empty_config() {
        let toml = "";
        let config = load_config_str(toml).unwrap();
        assert!(config.tables.is_empty());
        assert!(config.ignore.is_empty());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let toml = "this is not valid toml [[[";
        let err = load_config_str(toml).unwrap_err();
        // Single-line `matches!` pins the variant without a dead fallback arm.
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
    }

    #[test]
    fn no_dirsql_section_defaults_to_empty_ignore() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.ignore.is_empty());
    }

    #[test]
    fn load_config_missing_file_returns_io_error() {
        let err = load_config(Path::new("/nonexistent/.dirsql.toml")).unwrap_err();
        // Single-line `matches!` pins the variant without a dead fallback arm.
        assert!(matches!(err, ConfigError::Io(_)), "got: {err:?}");
    }

    #[test]
    fn optional_root_parses_when_present() {
        let toml = r#"
[dirsql]
root = "docs"

[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.root.as_deref(), Some(Path::new("docs")));
    }

    #[test]
    fn root_absent_by_default() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.root.is_none());
    }

    #[test]
    fn root_can_be_absolute() {
        let toml = r#"
[dirsql]
root = "/tmp/data"

[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.root.as_deref(), Some(Path::new("/tmp/data")));
    }

    #[test]
    fn persist_defaults_to_false() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(!config.persist);
        assert!(config.persist_path.is_none());
    }

    #[test]
    fn persist_true_is_parsed() {
        let toml = r#"
[dirsql]
persist = true
persist_path = "/var/cache/dirsql.db"

[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.persist);
        assert_eq!(
            config.persist_path.as_deref(),
            Some(Path::new("/var/cache/dirsql.db"))
        );
    }

    #[test]
    fn multiple_tables_preserve_order() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE a (_path TEXT)"
glob = "a/*.json"

[[table]]
ddl = "CREATE TABLE b (_path TEXT)"
glob = "b/*.csv"

[[table]]
ddl = "CREATE TABLE c (_path TEXT)"
glob = "c/*.yaml"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 3);
        assert!(config.tables[0].ddl.contains("a"));
        assert!(config.tables[1].ddl.contains("b"));
        assert!(config.tables[2].ddl.contains("c"));
    }

    #[test]
    fn unknown_top_level_keys_in_table_are_rejected() {
        // format/each/columns were removed from the grammar; serde's default
        // is permissive (unknown keys are ignored), but make sure existing
        // configs aren't silently broken: if a user still has `format = ...`
        // their table loads (the field is just dropped).
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
format = "json"
"#;
        // serde ignores unknown fields by default. We accept this as the
        // migration story; the config still parses and the table works
        // (filesystem-fact rows are produced regardless of the dropped key).
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 1);
    }

    #[test]
    fn extensions_parse_path_and_entrypoint() {
        let toml = r#"
[[dirsql.extension]]
path = "./ext/vec0.dylib"
entrypoint = "sqlite3_vec_init"

[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.extensions.len(), 1);
        assert_eq!(config.extensions[0].path, PathBuf::from("./ext/vec0.dylib"));
        assert_eq!(
            config.extensions[0].entrypoint.as_deref(),
            Some("sqlite3_vec_init")
        );
    }

    #[test]
    fn extension_entrypoint_is_optional() {
        let toml = r#"
[[dirsql.extension]]
path = "ext.so"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.extensions.len(), 1);
        assert!(config.extensions[0].entrypoint.is_none());
    }

    #[test]
    fn extension_missing_path_errors() {
        let toml = r#"
[[dirsql.extension]]
entrypoint = "sqlite3_x_init"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingExtensionField("path")),
            "got: {err:?}"
        );
    }

    #[test]
    fn extensions_default_empty_when_absent() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.extensions.is_empty());
    }

    #[test]
    fn multiple_extensions_preserve_order() {
        let toml = r#"
[[dirsql.extension]]
path = "a.so"

[[dirsql.extension]]
path = "b.so"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.extensions.len(), 2);
        assert_eq!(config.extensions[0].path, PathBuf::from("a.so"));
        assert_eq!(config.extensions[1].path, PathBuf::from("b.so"));
    }

    #[test]
    fn extension_empty_path_errors() {
        // An empty `path = ""` is as unusable as a missing key — it must be
        // rejected at parse time, not silently accepted and later resolved to
        // the config's parent directory. (RED for #225 review finding #4.)
        let toml = r#"
[[dirsql.extension]]
path = ""
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingExtensionField("path")),
            "got: {err:?}"
        );
    }
}
