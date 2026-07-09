use std::path::{Path, PathBuf};
use std::time::Duration;

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

    #[error("Field '{0}' must not be empty")]
    EmptyField(&'static str),

    #[error("Field '{field}' must be a positive number of seconds, got {value}")]
    InvalidTimeout { field: &'static str, value: i64 },

    #[error("Cannot combine an empty list of configs")]
    NoConfigs,

    #[error("Table '{name}' is defined by both {first} and {second}")]
    DuplicateTable {
        name: String,
        first: Source,
        second: Source,
    },

    #[error("Key '{key}' is set by both {first} and {second}")]
    ConflictingKey {
        key: &'static str,
        first: Source,
        second: Source,
    },
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// Where a config participating in [`combine_configs`] came from, so merge
/// conflict errors can name both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A config file path (e.g. `/proj/.dirsql.toml`).
    Path(PathBuf),
    /// A plugin package name (e.g. `dirsql-plugin-notes`).
    Package(String),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Path(path) => write!(f, "{}", path.display()),
            Source::Package(name) => f.write_str(name),
        }
    }
}

/// Merge multiple parsed configs into one.
///
/// Order-significant; at least one entry is required and a single entry is
/// returned unchanged. List-shaped config (`[[table]]`, `[[dirsql.extension]]`,
/// `ignore`) concatenates in input order. A table-name collision anywhere in
/// the combined set errors, naming both sources. Single-valued keys (`root`,
/// `pre-query`, `post-query`, `hook-timeout`) defined by more than
/// one config error, naming both sources — no silent shadowing, no precedence;
/// defined in exactly one config they merge through unchanged.
///
/// Tables whose DDL yields no parseable table name are concatenated without a
/// collision check; `Db::create_table` rejects them downstream.
///
/// Plugin whitelist enforcement ("a fragment may not set `root`") is the
/// discovery layer's job, per fragment, before calling this.
pub fn combine_configs(configs: &[(Source, Config)]) -> Result<Config> {
    let (first, rest) = configs.split_first().ok_or(ConfigError::NoConfigs)?;
    if rest.is_empty() {
        return Ok(first.1.clone());
    }

    let mut tables = Vec::new();
    let mut ignore = Vec::new();
    let mut extensions = Vec::new();
    let mut table_sources: std::collections::HashMap<String, &Source> =
        std::collections::HashMap::new();

    let mut root: Option<(&Source, PathBuf)> = None;
    let mut pre_query: Option<(&Source, String)> = None;
    let mut post_query: Option<(&Source, String)> = None;
    let mut hook_timeout: Option<(&Source, Duration)> = None;

    for (source, config) in configs {
        for table in &config.tables {
            if let Some(name) = crate::db::parse_table_name(&table.ddl)
                && let Some(prior) = table_sources.insert(name.clone(), source)
            {
                return Err(ConfigError::DuplicateTable {
                    name,
                    first: prior.clone(),
                    second: source.clone(),
                });
            }
            tables.push(table.clone());
        }
        ignore.extend(config.ignore.iter().cloned());
        extensions.extend(config.extensions.iter().cloned());

        merge_single("root", &mut root, config.root.as_ref(), source)?;
        merge_single(
            "pre-query",
            &mut pre_query,
            config.pre_query.as_ref(),
            source,
        )?;
        merge_single(
            "post-query",
            &mut post_query,
            config.post_query.as_ref(),
            source,
        )?;
        merge_single(
            "hook-timeout",
            &mut hook_timeout,
            config.hook_timeout.as_ref(),
            source,
        )?;
    }

    Ok(Config {
        root: root.map(|(_, value)| value),
        ignore,
        tables,
        extensions,
        pre_query: pre_query.map(|(_, value)| value),
        post_query: post_query.map(|(_, value)| value),
        hook_timeout: hook_timeout.map(|(_, value)| value),
    })
}

/// Fold one config's value for a single-valued key into the merge `slot`,
/// erroring when a prior config already defined it.
fn merge_single<'a, T: Clone>(
    key: &'static str,
    slot: &mut Option<(&'a Source, T)>,
    value: Option<&T>,
    source: &'a Source,
) -> Result<()> {
    if let Some(value) = value {
        if let Some((prior, _)) = slot {
            return Err(ConfigError::ConflictingKey {
                key,
                first: (*prior).clone(),
                second: source.clone(),
            });
        }
        *slot = Some((source, value.clone()));
    }
    Ok(())
}

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
    /// SQLite extensions to load at startup, declared via
    /// `[[dirsql.extension]]`. Paths are taken verbatim from the file here;
    /// relative paths are resolved against the config file's parent directory
    /// by the caller (`DirSQLBuilder::resolve`).
    pub extensions: Vec<ExtensionSpec>,
    /// Optional server-wide `pre-query` command (`[dirsql].pre-query`). When
    /// set, the HTTP server passes each `POST /query` request body to this
    /// command as `{args}` and runs the plain-text SQL it prints, instead of
    /// parsing the body as `{"sql": …}`. See `dirsql::command` for the
    /// execution contract. Only the CLI server consults this; the SDK ignores
    /// it.
    pub pre_query: Option<String>,
    /// Optional server-wide `post-query` command (`[dirsql].post-query`). When
    /// set, the HTTP server hands each successful `POST /query` result set (the
    /// rows serialized as a JSON array) to this command as `{args}` and on
    /// stdin, and returns the JSON body the command prints, instead of returning
    /// the rows as-is. See `dirsql::command` for the execution contract. Only
    /// the CLI server consults this; the SDK ignores it.
    pub post_query: Option<String>,
    /// Optional timeout for every command-backed hook — `on-file`, `pre-query`,
    /// and `post-query` alike (`[dirsql].hook-timeout`, positive seconds). One
    /// global bound rather than a per-hook knob. When absent, hooks fall back to
    /// the shared 30-second default ([`crate::command::DEFAULT_COMMAND_TIMEOUT`]).
    pub hook_timeout: Option<Duration>,
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
/// (`path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`).
/// Content interpretation (frontmatter, JSON dot-paths, CSV parsing, etc.)
/// is intentionally out of scope; for that, register a programmatic
/// [`crate::Table`] with your own extract closure.
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub ddl: String,
    pub glob: String,
    pub strict: Option<bool>,
    /// Optional per-file command (`on-file`). When set, each matched file's
    /// rows come from running this command (which reads the file and prints a
    /// JSON array of row objects) instead of the empty filesystem-facts-only
    /// row. See `dirsql::command` for the execution contract.
    pub on_file: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    dirsql: Option<RawDirsql>,
    table: Option<Vec<RawTable>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawDirsql {
    root: Option<PathBuf>,
    ignore: Option<Vec<String>>,
    extension: Option<Vec<RawExtension>>,
    #[serde(rename = "pre-query")]
    pre_query: Option<String>,
    #[serde(rename = "post-query")]
    post_query: Option<String>,
    #[serde(rename = "hook-timeout")]
    hook_timeout: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtension {
    path: Option<String>,
    entrypoint: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTable {
    ddl: Option<String>,
    glob: Option<String>,
    strict: Option<bool>,
    #[serde(rename = "on-file")]
    on_file: Option<String>,
}

/// Load and parse a `.dirsql.toml` config file from the given path.
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    load_config_str(&content)
}

/// Parse a `.dirsql.toml` config from a string (useful for testing).
pub fn load_config_str(content: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(content)?;

    let d = raw.dirsql.unwrap_or_default();
    let root = d.root;
    let ignore = d.ignore.unwrap_or_default();
    let raw_extensions = d.extension.unwrap_or_default();
    let raw_pre_query = d.pre_query;
    let raw_post_query = d.post_query;
    let hook_timeout = parse_timeout_secs("hook-timeout", d.hook_timeout)?;

    // Present-but-empty hook commands are rejected at parse time rather than
    // spawning an empty command later.
    let pre_query = match raw_pre_query {
        Some(cmd) if cmd.trim().is_empty() => {
            return Err(ConfigError::EmptyField("pre-query"));
        }
        other => other,
    };

    let post_query = match raw_post_query {
        Some(cmd) if cmd.trim().is_empty() => {
            return Err(ConfigError::EmptyField("post-query"));
        }
        other => other,
    };

    let mut extensions = Vec::with_capacity(raw_extensions.len());
    for raw_ext in raw_extensions {
        // An empty `path = ""` is rejected at parse time rather than silently
        // resolving to a directory later.
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

        let on_file = match raw_table.on_file {
            Some(cmd) if cmd.trim().is_empty() => {
                return Err(ConfigError::EmptyField("on-file"));
            }
            other => other,
        };

        tables.push(TableConfig {
            ddl,
            glob,
            strict: raw_table.strict,
            on_file,
        });
    }

    Ok(Config {
        root,
        ignore,
        tables,
        extensions,
        pre_query,
        post_query,
        hook_timeout,
    })
}

/// Validate an optional timeout config value (whole seconds) into a
/// [`Duration`], rejecting zero and negative values at parse time.
fn parse_timeout_secs(field: &'static str, raw: Option<i64>) -> Result<Option<Duration>> {
    match raw {
        Some(secs) if secs <= 0 => Err(ConfigError::InvalidTimeout { field, value: secs }),
        Some(secs) => Ok(Some(Duration::from_secs(secs as u64))),
        None => Ok(None),
    }
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
ddl = "CREATE TABLE comments (thread_id TEXT, path TEXT)"
glob = "_comments/{thread_id}/index.jsonl"

[[table]]
ddl = "CREATE TABLE items (path TEXT, size INTEGER)"
glob = "catalog/*.json"
strict = true
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.ignore, vec!["node_modules/**", ".git/**"]);
        assert_eq!(config.tables.len(), 2);

        let t0 = &config.tables[0];
        assert_eq!(t0.ddl, "CREATE TABLE comments (thread_id TEXT, path TEXT)");
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
        assert!(matches!(err, ConfigError::Io(_)), "got: {err:?}");
    }

    #[test]
    fn optional_root_parses_when_present() {
        let toml = r#"
[dirsql]
root = "docs"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.root.as_deref(), Some(Path::new("docs")));
    }

    #[test]
    fn root_absent_by_default() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
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
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.root.as_deref(), Some(Path::new("/tmp/data")));
    }

    #[test]
    fn persist_key_is_rejected_as_unknown() {
        // Persistence moved to the `--persist` CLI flag; the TOML key is gone.
        let toml = r#"
[dirsql]
persist = true
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("persist"), "got: {err}");
    }

    #[test]
    fn persist_path_key_is_rejected_as_unknown() {
        let toml = r#"
[dirsql]
persist_path = "/var/cache/dirsql.db"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("persist_path"), "got: {err}");
    }

    #[test]
    fn multiple_tables_preserve_order() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE a (path TEXT)"
glob = "a/*.json"

[[table]]
ddl = "CREATE TABLE b (path TEXT)"
glob = "b/*.csv"

[[table]]
ddl = "CREATE TABLE c (path TEXT)"
glob = "c/*.yaml"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 3);
        assert!(config.tables[0].ddl.contains("a"));
        assert!(config.tables[1].ddl.contains("b"));
        assert!(config.tables[2].ddl.contains("c"));
    }

    #[test]
    fn unknown_key_in_table_is_rejected() {
        // An unknown `[[table]]` key (a removed key like `format`, or a typo)
        // is a hard parse error naming the key, never silently dropped.
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
format = "json"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("format"), "got: {err}");
    }

    #[test]
    fn unknown_key_in_dirsql_section_is_rejected() {
        // A misspelled `[dirsql]` key errors rather than silently no-opping.
        let toml = r#"
[dirsql]
persistpath = "cache.db"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("persistpath"), "got: {err}");
    }

    #[test]
    fn unknown_top_level_key_is_rejected() {
        // A stray key outside any known section (`glbo` for `[dirsql]`) errors.
        let toml = r#"
glbo = "typo"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("glbo"), "got: {err}");
    }

    #[test]
    fn unknown_key_in_extension_is_rejected() {
        // A misspelled `[[dirsql.extension]]` key (`entrypont`) errors.
        let toml = r#"
[[dirsql.extension]]
path = "vec0.so"
entrypont = "sqlite3_vec_init"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("entrypont"), "got: {err}");
    }

    #[test]
    fn extensions_parse_path_and_entrypoint() {
        let toml = r#"
[[dirsql.extension]]
path = "./ext/vec0.dylib"
entrypoint = "sqlite3_vec_init"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
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
ddl = "CREATE TABLE t (path TEXT)"
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
    fn on_file_parses_when_present() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob = "**/meta.json"
on-file = "uv run python extract_papers.py {path}"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 1);
        assert_eq!(
            config.tables[0].on_file.as_deref(),
            Some("uv run python extract_papers.py {path}")
        );
    }

    #[test]
    fn on_file_absent_is_none() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.tables[0].on_file.is_none());
    }

    #[test]
    fn on_file_empty_errors() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
on-file = "   "
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyField("on-file")),
            "got: {err:?}"
        );
    }

    #[test]
    fn pre_query_parses_when_present() {
        let toml = r#"
[dirsql]
pre-query = "uv run python to_sql.py {args}"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(
            config.pre_query.as_deref(),
            Some("uv run python to_sql.py {args}")
        );
    }

    #[test]
    fn pre_query_absent_is_none() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.pre_query.is_none());
    }

    #[test]
    fn pre_query_empty_errors() {
        let toml = r#"
[dirsql]
pre-query = "   "
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyField("pre-query")),
            "got: {err:?}"
        );
    }

    #[test]
    fn post_query_parses_when_present() {
        let toml = r#"
[dirsql]
post-query = "jq '{results: .}'"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.post_query.as_deref(), Some("jq '{results: .}'"));
    }

    #[test]
    fn post_query_absent_is_none() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.post_query.is_none());
    }

    #[test]
    fn post_query_empty_errors() {
        let toml = r#"
[dirsql]
post-query = "   "
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::EmptyField("post-query")),
            "got: {err:?}"
        );
    }

    fn src(label: &str) -> Source {
        Source::Path(PathBuf::from(label))
    }

    fn cfg(toml: &str) -> Config {
        load_config_str(toml).unwrap()
    }

    #[test]
    fn combine_empty_slice_rejected() {
        let err = combine_configs(&[]).unwrap_err();
        assert!(matches!(err, ConfigError::NoConfigs), "got: {err:?}");
    }

    #[test]
    fn combine_singleton_returns_config_unchanged() {
        let config = cfg(r#"
[dirsql]
root = "docs"
ignore = ["*.tmp"]
pre-query = "to_sql {args}"
post-query = "jq '{results: .}'"

[[dirsql.extension]]
path = "vec0.so"
entrypoint = "sqlite3_vec_init"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#);
        let merged = combine_configs(&[(src("/proj/.dirsql.toml"), config.clone())]).unwrap();
        assert_eq!(merged.root, config.root);
        assert_eq!(merged.ignore, config.ignore);
        assert_eq!(merged.extensions, config.extensions);
        assert_eq!(merged.pre_query, config.pre_query);
        assert_eq!(merged.post_query, config.post_query);
        assert_eq!(merged.tables.len(), 1);
        assert_eq!(merged.tables[0].ddl, config.tables[0].ddl);
        assert_eq!(merged.tables[0].glob, config.tables[0].glob);
    }

    #[test]
    fn combine_concatenates_tables_in_input_order() {
        let a = cfg(r#"
[[table]]
ddl = "CREATE TABLE a (path TEXT)"
glob = "a/*.json"

[[table]]
ddl = "CREATE TABLE b (path TEXT)"
glob = "b/*.json"
"#);
        let b = cfg(r#"
[[table]]
ddl = "CREATE TABLE c (path TEXT)"
glob = "c/*.json"
"#);
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        let ddls: Vec<&str> = merged.tables.iter().map(|t| t.ddl.as_str()).collect();
        assert_eq!(
            ddls,
            vec![
                "CREATE TABLE a (path TEXT)",
                "CREATE TABLE b (path TEXT)",
                "CREATE TABLE c (path TEXT)",
            ]
        );
        assert_eq!(merged.tables[2].glob, "c/*.json");
    }

    #[test]
    fn combine_concatenates_ignore_in_input_order() {
        let a = cfg("[dirsql]\nignore = [\"a/**\", \"b/**\"]\n");
        let b = cfg("[dirsql]\nignore = [\"c/**\"]\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(merged.ignore, vec!["a/**", "b/**", "c/**"]);
    }

    #[test]
    fn combine_concatenates_extensions_in_input_order() {
        let a = cfg("[[dirsql.extension]]\npath = \"a.so\"\n");
        let b = cfg("[[dirsql.extension]]\npath = \"b.so\"\nentrypoint = \"init_b\"\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(
            merged.extensions,
            vec![
                ExtensionSpec {
                    path: PathBuf::from("a.so"),
                    entrypoint: None,
                },
                ExtensionSpec {
                    path: PathBuf::from("b.so"),
                    entrypoint: Some("init_b".to_string()),
                },
            ]
        );
    }

    #[test]
    fn combine_duplicate_table_name_errors_naming_both_sources() {
        let a = cfg("[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\n");
        let b = cfg("[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\n");
        let err = combine_configs(&[
            (src("/proj/.dirsql.toml"), a),
            (src("/plugin/frag.toml"), b),
        ])
        .unwrap_err();
        match &err {
            ConfigError::DuplicateTable {
                name,
                first,
                second,
            } => {
                assert_eq!(name, "t");
                assert_eq!(first, &src("/proj/.dirsql.toml"));
                assert_eq!(second, &src("/plugin/frag.toml"));
            }
            other => panic!("got: {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("'t'"), "got: {msg}");
        assert!(msg.contains("/proj/.dirsql.toml"), "got: {msg}");
        assert!(msg.contains("/plugin/frag.toml"), "got: {msg}");
    }

    #[test]
    fn combine_singleton_with_internal_duplicate_returns_unchanged() {
        // The single-entry identity path runs no collision check, exactly like
        // a plain single-config load.
        let config = cfg(concat!(
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\n",
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\n",
        ));
        let merged = combine_configs(&[(src("/a"), config)]).unwrap();
        assert_eq!(merged.tables.len(), 2);
    }

    #[test]
    fn combine_intra_config_duplicate_in_multi_merge_errors() {
        let a = cfg(concat!(
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\n",
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\n",
        ));
        let b = cfg("[dirsql]\nignore = [\"c/**\"]\n");
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        match &err {
            ConfigError::DuplicateTable {
                name,
                first,
                second,
            } => {
                assert_eq!(name, "t");
                assert_eq!(first, &src("/a"));
                assert_eq!(second, &src("/a"));
            }
            other => panic!("got: {other:?}"),
        }
    }

    #[test]
    fn combine_duplicate_table_name_detected_through_quoting() {
        let a = cfg("[[table]]\nddl = 'CREATE TABLE \"t\" (x TEXT)'\nglob = \"a/*.json\"\n");
        let b = cfg("[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\n");
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        assert!(
            matches!(&err, ConfigError::DuplicateTable { name, .. } if name == "t"),
            "got: {err:?}"
        );
    }

    #[test]
    fn combine_unparseable_ddl_concatenates_without_collision_check() {
        let a = cfg("[[table]]\nddl = \"not a create table\"\nglob = \"a/*.json\"\n");
        let b = cfg("[[table]]\nddl = \"also not a create table\"\nglob = \"b/*.json\"\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(merged.tables.len(), 2);
    }

    #[test]
    fn combine_pre_query_in_two_configs_errors_naming_both_sources() {
        let a = cfg("[dirsql]\npre-query = \"to_sql_a {args}\"\n");
        let b = cfg("[dirsql]\npre-query = \"to_sql_b {args}\"\n");
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        match &err {
            ConfigError::ConflictingKey { key, first, second } => {
                assert_eq!(*key, "pre-query");
                assert_eq!(first, &src("/a"));
                assert_eq!(second, &src("/b"));
            }
            other => panic!("got: {other:?}"),
        }
    }

    #[test]
    fn combine_post_query_in_two_configs_errors_naming_both_sources() {
        let a = cfg("[dirsql]\npost-query = \"jq_a\"\n");
        let b = cfg("[dirsql]\npost-query = \"jq_b\"\n");
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        match &err {
            ConfigError::ConflictingKey { key, first, second } => {
                assert_eq!(*key, "post-query");
                assert_eq!(first, &src("/a"));
                assert_eq!(second, &src("/b"));
            }
            other => panic!("got: {other:?}"),
        }
    }

    #[test]
    fn combine_hooks_in_one_config_merge_through() {
        let a = cfg("[dirsql]\npre-query = \"to_sql {args}\"\npost-query = \"jq -c .\"\n");
        let b = cfg("[dirsql]\nignore = [\"c/**\"]\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(merged.pre_query.as_deref(), Some("to_sql {args}"));
        assert_eq!(merged.post_query.as_deref(), Some("jq -c ."));
    }

    #[test]
    fn combine_root_in_two_configs_errors_naming_both_sources() {
        let a = cfg("[dirsql]\nroot = \"docs\"\n");
        let b = cfg("[dirsql]\nroot = \"data\"\n");
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        match &err {
            ConfigError::ConflictingKey { key, first, second } => {
                assert_eq!(*key, "root");
                assert_eq!(first, &src("/a"));
                assert_eq!(second, &src("/b"));
            }
            other => panic!("got: {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("'root'"), "got: {msg}");
        assert!(msg.contains("/a"), "got: {msg}");
        assert!(msg.contains("/b"), "got: {msg}");
    }

    #[test]
    fn combine_root_in_one_config_merges_through() {
        let a = cfg("[dirsql]\nignore = [\"a/**\"]\n");
        let b = cfg("[dirsql]\nroot = \"docs\"\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(merged.root.as_deref(), Some(Path::new("docs")));
    }

    #[test]
    fn combine_error_display_includes_package_source_verbatim() {
        let a = cfg("[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\n");
        let b = cfg("[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\n");
        let err = combine_configs(&[
            (src("/proj/.dirsql.toml"), a),
            (Source::Package("dirsql-plugin-notes".to_string()), b),
        ])
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/proj/.dirsql.toml"), "got: {msg}");
        assert!(msg.contains("dirsql-plugin-notes"), "got: {msg}");
    }

    #[test]
    fn combine_three_configs_concatenates_across_all() {
        let a = cfg(
            "[dirsql]\nignore = [\"a/**\"]\n\n[[table]]\nddl = \"CREATE TABLE a (x TEXT)\"\nglob = \"a/*\"\n",
        );
        let b = cfg("[[table]]\nddl = \"CREATE TABLE b (x TEXT)\"\nglob = \"b/*\"\n");
        let c = cfg(
            "[dirsql]\nignore = [\"c/**\"]\n\n[[table]]\nddl = \"CREATE TABLE c (x TEXT)\"\nglob = \"c/*\"\n",
        );
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b), (src("/c"), c)]).unwrap();
        assert_eq!(merged.ignore, vec!["a/**", "c/**"]);
        let ddls: Vec<&str> = merged.tables.iter().map(|t| t.ddl.as_str()).collect();
        assert_eq!(
            ddls,
            vec![
                "CREATE TABLE a (x TEXT)",
                "CREATE TABLE b (x TEXT)",
                "CREATE TABLE c (x TEXT)",
            ]
        );
    }

    #[test]
    fn source_display_path_and_package() {
        assert_eq!(src("/proj/.dirsql.toml").to_string(), "/proj/.dirsql.toml");
        assert_eq!(
            Source::Package("dirsql-plugin-notes".to_string()).to_string(),
            "dirsql-plugin-notes"
        );
    }

    #[test]
    fn hook_timeout_parses_to_duration_seconds() {
        let toml = r#"
[dirsql]
hook-timeout = 300
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.hook_timeout, Some(Duration::from_secs(300)));
    }

    #[test]
    fn hook_timeout_absent_is_none() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.json"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(config.hook_timeout.is_none());
    }

    #[test]
    fn hook_timeout_zero_errors() {
        let toml = r#"
[dirsql]
hook-timeout = 0
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::InvalidTimeout {
                    field: "hook-timeout",
                    value: 0
                }
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn hook_timeout_negative_errors() {
        let toml = r#"
[dirsql]
hook-timeout = -5
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::InvalidTimeout {
                    field: "hook-timeout",
                    value: -5
                }
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn invalid_timeout_error_names_the_field_and_value() {
        let err = ConfigError::InvalidTimeout {
            field: "hook-timeout",
            value: -1,
        };
        assert_eq!(
            err.to_string(),
            "Field 'hook-timeout' must be a positive number of seconds, got -1"
        );
    }

    #[test]
    fn extension_empty_path_errors() {
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
