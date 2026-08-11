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

    #[error("Missing required field '{0}' in [[dirsql.function]] entry")]
    MissingFunctionField(&'static str),

    #[error(
        "[[dirsql.function]] '{name}' is not a valid SQL function name: use an \
         ASCII letter or underscore followed by letters, digits, or underscores"
    )]
    InvalidFunctionName { name: String },

    #[error(
        "[[dirsql.function]] '{name}': 'args' must list at least one accepted \
         arity (e.g. args = [1])"
    )]
    EmptyFunctionArgs { name: String },

    #[error("[[dirsql.function]] '{name}': arity {value} is out of range (0..=127)")]
    InvalidFunctionArity { name: String, value: i64 },

    #[error("[[dirsql.function]] '{name}': arity {value} is listed more than once in 'args'")]
    DuplicateFunctionArity { name: String, value: i64 },

    #[error(
        "[[dirsql.function]] '{name}': 'timeout' must be positive whole seconds \
         (timeout = 600) or a duration string like \"600s\" or \"250ms\", got {value}"
    )]
    InvalidFunctionTimeout { name: String, value: String },

    #[error("Field '{0}' must not be empty")]
    EmptyField(&'static str),

    #[error(
        "[[table]] '{glob}' has no on-file hook, so every row would be all-NULL. \
         Add an `on-file` hook that emits the columns, or, for stat columns with \
         no code, query the path directly: `FROM './'`"
    )]
    HooklessTable { glob: String },

    #[error(
        "'hook-timeout' has been removed: `on-file` hooks run unbounded. Delete the \
         key from [dirsql]; to bound a hook, wrap its command in timeout(1) \
         (on-file = \"timeout 30 my-extractor {{path}}\"). A [[dirsql.function]] \
         entry bounds each worker call with its own `timeout` key (default 30s)."
    )]
    RemovedHookTimeout,

    #[error("Cannot combine an empty list of configs")]
    NoConfigs,

    #[error("Table '{name}' is defined by both {first} and {second}")]
    DuplicateTable {
        name: String,
        first: Source,
        second: Source,
    },

    #[error("Function '{name}' is declared by both {first} and {second}")]
    DuplicateFunction {
        name: String,
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
/// `[[dirsql.function]]`, `ignore`) concatenates in input order. A table-name
/// or function-name collision anywhere in the combined set errors, naming both
/// sources.
///
/// Tables whose DDL yields no parseable table name are concatenated without a
/// collision check; `Db::create_table` rejects them downstream.
pub fn combine_configs(configs: &[(Source, Config)]) -> Result<Config> {
    let (first, rest) = configs.split_first().ok_or(ConfigError::NoConfigs)?;
    if rest.is_empty() {
        return Ok(first.1.clone());
    }

    let mut tables = Vec::new();
    let mut ignore = Vec::new();
    let mut extensions = Vec::new();
    let mut functions = Vec::new();
    let mut table_sources: std::collections::HashMap<String, &Source> =
        std::collections::HashMap::new();
    let mut function_sources: std::collections::HashMap<String, &Source> =
        std::collections::HashMap::new();

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
        for function in &config.functions {
            if let Some(prior) = function_sources.insert(function.name.clone(), source) {
                return Err(ConfigError::DuplicateFunction {
                    name: function.name.clone(),
                    first: prior.clone(),
                    second: source.clone(),
                });
            }
            functions.push(function.clone());
        }
    }

    Ok(Config {
        ignore,
        tables,
        extensions,
        functions,
    })
}

/// Parsed configuration from a `.dirsql.toml` file.
#[derive(Debug, Clone)]
pub struct Config {
    pub ignore: Vec<String>,
    pub tables: Vec<TableConfig>,
    /// SQLite extensions to load at startup, declared via
    /// `[[dirsql.extension]]`. Paths are taken verbatim from the file here;
    /// relative paths are resolved against the config file's parent directory
    /// by the caller (`DirSQLBuilder::resolve`).
    pub extensions: Vec<ExtensionSpec>,
    /// Worker-backed SQL scalar functions, declared via `[[dirsql.function]]`.
    /// Registered on the connection at startup; nothing runs until a query
    /// calls one. See [`FunctionSpec`].
    pub functions: Vec<FunctionSpec>,
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

/// A worker-backed SQL scalar function declared via `[[dirsql.function]]`.
///
/// The function is registered on the connection once per accepted arity
/// (SQLite supports same-name multi-arity registration). Registration is
/// inert: the `command` worker process is spawned lazily on the function's
/// first call and kept alive for the rest of the invocation, speaking
/// newline-delimited JSON over stdin/stdout (see `dirsql::functions`).
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionSpec {
    /// The SQL name queries call the function by.
    pub name: String,
    /// Accepted arities; the function is registered once per entry.
    pub args: Vec<u8>,
    /// The worker command template. Spawned (argv-split, no shell) from the
    /// config file's directory on the function's first call.
    pub command: String,
    /// When true, the function is registered with `SQLITE_DETERMINISTIC`.
    pub deterministic: bool,
    /// Optional per-round-trip-call timeout, overriding the function
    /// mechanism's own 30-second default.
    pub timeout: Option<Duration>,
}

/// Configuration for a single table.
///
/// A config-defined table maps a glob pattern to a SQL DDL. Each matched
/// file produces one row whose columns are derived from filesystem facts:
/// glob path captures (named `{placeholder}` segments) and stat virtuals
/// (`path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`).
/// Content interpretation (frontmatter, JSON dot-paths, CSV parsing, etc.)
/// is intentionally out of scope; for that, register a programmatic
/// [`crate::Table`] with your own on-file callback.
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub ddl: String,
    pub glob: String,
    pub strict: Option<bool>,
    /// The required per-file command (`on-file`). Each matched file's rows come
    /// from running this command, which reads the file and prints a JSON array
    /// of row objects. A table without it would emit no columns of its own —
    /// every row all-NULL — so it is rejected at load. See `dirsql::command`
    /// for the execution contract.
    pub on_file: String,
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
    ignore: Option<Vec<String>>,
    extension: Option<Vec<RawExtension>>,
    function: Option<Vec<RawFunction>>,
    // Removed key, still deserialized (any shape) so a config declaring it
    // hits the dedicated actionable error, not the generic unknown-key one.
    #[serde(rename = "hook-timeout")]
    hook_timeout: Option<toml::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFunction {
    name: Option<String>,
    args: Option<Vec<i64>>,
    command: Option<String>,
    deterministic: Option<bool>,
    timeout: Option<RawFunctionTimeout>,
}

/// The `timeout` key accepts positive whole seconds (`timeout = 600`) or a
/// suffixed duration string (`timeout = "600s"`, `"250ms"`).
#[derive(Deserialize)]
#[serde(untagged)]
enum RawFunctionTimeout {
    Secs(i64),
    Text(String),
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
    if d.hook_timeout.is_some() {
        return Err(ConfigError::RemovedHookTimeout);
    }
    let ignore = d.ignore.unwrap_or_default();
    let raw_extensions = d.extension.unwrap_or_default();

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

    let mut functions = Vec::new();
    for raw_function in d.function.unwrap_or_default() {
        functions.push(parse_function(raw_function)?);
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
            Some(cmd) => cmd,
            None => return Err(ConfigError::HooklessTable { glob }),
        };

        tables.push(TableConfig {
            ddl,
            glob,
            strict: raw_table.strict,
            on_file,
        });
    }

    Ok(Config {
        ignore,
        tables,
        extensions,
        functions,
    })
}

/// Validate one `[[dirsql.function]]` entry into a [`FunctionSpec`].
fn parse_function(raw: RawFunction) -> Result<FunctionSpec> {
    let name = raw
        .name
        .filter(|n| !n.is_empty())
        .ok_or(ConfigError::MissingFunctionField("name"))?;
    if !is_valid_function_name(&name) {
        return Err(ConfigError::InvalidFunctionName { name });
    }

    let command = match raw.command {
        Some(cmd) if cmd.trim().is_empty() => {
            return Err(ConfigError::MissingFunctionField("command"));
        }
        Some(cmd) => cmd,
        None => return Err(ConfigError::MissingFunctionField("command")),
    };

    let raw_args = raw.args.ok_or(ConfigError::MissingFunctionField("args"))?;
    if raw_args.is_empty() {
        return Err(ConfigError::EmptyFunctionArgs { name });
    }
    let mut args = Vec::with_capacity(raw_args.len());
    for value in raw_args {
        // SQLite caps function arity at 127.
        let arity = u8::try_from(value)
            .ok()
            .filter(|a| *a <= 127)
            .ok_or_else(|| ConfigError::InvalidFunctionArity {
                name: name.clone(),
                value,
            })?;
        if args.contains(&arity) {
            return Err(ConfigError::DuplicateFunctionArity {
                name: name.clone(),
                value,
            });
        }
        args.push(arity);
    }

    let timeout = match raw.timeout {
        None => None,
        Some(raw_timeout) => Some(parse_function_timeout(&name, &raw_timeout)?),
    };

    Ok(FunctionSpec {
        name,
        args,
        command,
        deterministic: raw.deterministic.unwrap_or(false),
        timeout,
    })
}

/// Whether `name` is registrable and callable as an unquoted SQL function
/// name: an ASCII letter or underscore followed by ASCII letters, digits, or
/// underscores.
fn is_valid_function_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate a `[[dirsql.function]]` `timeout` value into a [`Duration`]:
/// positive whole seconds, or a positive-integer string suffixed `s` or `ms`.
fn parse_function_timeout(name: &str, raw: &RawFunctionTimeout) -> Result<Duration> {
    let invalid = |value: String| ConfigError::InvalidFunctionTimeout {
        name: name.to_string(),
        value,
    };
    match raw {
        RawFunctionTimeout::Secs(secs) if *secs > 0 => Ok(Duration::from_secs(
            u64::try_from(*secs).expect("positive i64 fits in u64"),
        )),
        RawFunctionTimeout::Secs(secs) => Err(invalid(secs.to_string())),
        RawFunctionTimeout::Text(text) => {
            let (digits, from_int): (&str, fn(u64) -> Duration) =
                if let Some(digits) = text.strip_suffix("ms") {
                    (digits, Duration::from_millis)
                } else if let Some(digits) = text.strip_suffix('s') {
                    (digits, Duration::from_secs)
                } else {
                    return Err(invalid(format!("{text:?}")));
                };
            match digits.parse::<u64>() {
                Ok(value) if value > 0 => Ok(from_int(value)),
                _ => Err(invalid(format!("{text:?}"))),
            }
        }
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
on-file = "cat {path}"

[[table]]
ddl = "CREATE TABLE items (path TEXT, size INTEGER)"
glob = "catalog/*.json"
on-file = "cat {path}"
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
on-file = "cat {path}"
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
on-file = "cat {path}"
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
on-file = "cat {path}"

[[table]]
ddl = "CREATE TABLE b (path TEXT)"
glob = "b/*.csv"
on-file = "cat {path}"

[[table]]
ddl = "CREATE TABLE c (path TEXT)"
glob = "c/*.yaml"
on-file = "cat {path}"
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
on-file = "cat {path}"
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
    fn root_key_in_dirsql_section_is_rejected() {
        // `root` is no longer a config key (#540): the runner owns the index
        // root. An old config carrying it fails loudly, naming the key.
        let toml = r#"
[dirsql]
root = "docs"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("root"), "got: {err}");
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
on-file = "cat {path}"
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
on-file = "cat {path}"
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
            config.tables[0].on_file,
            "uv run python extract_papers.py {path}"
        );
    }

    #[test]
    fn on_file_absent_is_a_hookless_load_error() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(&err, ConfigError::HooklessTable { glob } if glob == "*.json"),
            "got: {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("on-file"), "got: {msg}");
        assert!(msg.contains("FROM './'"), "got: {msg}");
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
    fn pre_query_key_is_rejected_as_unknown() {
        // The `pre-query` hook is removed (#803); the key errors like any
        // other unknown key, naming it.
        let toml = r#"
[dirsql]
pre-query = "uv run python to_sql.py {args}"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("pre-query"), "got: {err}");
    }

    #[test]
    fn post_query_key_is_rejected_as_unknown() {
        // Same removal contract for `post-query` (#803).
        let toml = r#"
[dirsql]
post-query = "jq '{results: .}'"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("post-query"), "got: {err}");
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
ignore = ["*.tmp"]

[[dirsql.extension]]
path = "vec0.so"
entrypoint = "sqlite3_vec_init"

[[table]]
ddl = "CREATE TABLE t (path TEXT)"
glob = "*.json"
on-file = "cat {path}"
"#);
        let merged = combine_configs(&[(src("/proj/.dirsql.toml"), config.clone())]).unwrap();
        assert_eq!(merged.ignore, config.ignore);
        assert_eq!(merged.extensions, config.extensions);
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
on-file = "cat {path}"

[[table]]
ddl = "CREATE TABLE b (path TEXT)"
glob = "b/*.json"
on-file = "cat {path}"
"#);
        let b = cfg(r#"
[[table]]
ddl = "CREATE TABLE c (path TEXT)"
glob = "c/*.json"
on-file = "cat {path}"
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
        let a = cfg(
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
        );
        let b = cfg(
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
        );
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
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
        ));
        let merged = combine_configs(&[(src("/a"), config)]).unwrap();
        assert_eq!(merged.tables.len(), 2);
    }

    #[test]
    fn combine_intra_config_duplicate_in_multi_merge_errors() {
        let a = cfg(concat!(
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
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
        let a = cfg(
            "[[table]]\nddl = 'CREATE TABLE \"t\" (x TEXT)'\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
        );
        let b = cfg(
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
        );
        let err = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap_err();
        assert!(
            matches!(&err, ConfigError::DuplicateTable { name, .. } if name == "t"),
            "got: {err:?}"
        );
    }

    #[test]
    fn combine_unparseable_ddl_concatenates_without_collision_check() {
        let a = cfg(
            "[[table]]\nddl = \"not a create table\"\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
        );
        let b = cfg(
            "[[table]]\nddl = \"also not a create table\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
        );
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        assert_eq!(merged.tables.len(), 2);
    }

    #[test]
    fn combine_error_display_includes_package_source_verbatim() {
        let a = cfg(
            "[[table]]\nddl = \"CREATE TABLE t (x TEXT)\"\nglob = \"a/*.json\"\non-file = \"cat {path}\"\n",
        );
        let b = cfg(
            "[[table]]\nddl = \"CREATE TABLE t (y TEXT)\"\nglob = \"b/*.json\"\non-file = \"cat {path}\"\n",
        );
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
            "[dirsql]\nignore = [\"a/**\"]\n\n[[table]]\nddl = \"CREATE TABLE a (x TEXT)\"\nglob = \"a/*\"\non-file = \"cat {path}\"\n",
        );
        let b = cfg(
            "[[table]]\nddl = \"CREATE TABLE b (x TEXT)\"\nglob = \"b/*\"\non-file = \"cat {path}\"\n",
        );
        let c = cfg(
            "[dirsql]\nignore = [\"c/**\"]\n\n[[table]]\nddl = \"CREATE TABLE c (x TEXT)\"\nglob = \"c/*\"\non-file = \"cat {path}\"\n",
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
    fn hook_timeout_key_is_rejected_with_the_replacement_idiom() {
        // The key is removed; the error must name the timeout(1) wrapper
        // idiom and the function-level `timeout` replacement.
        let toml = r#"
[dirsql]
hook-timeout = 120
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::RemovedHookTimeout),
            "got: {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("hook-timeout"), "got: {msg}");
        assert!(msg.contains("timeout 30 my-extractor {path}"), "got: {msg}");
        assert!(msg.contains("timeout(1)"), "got: {msg}");
        assert!(msg.contains("[[dirsql.function]]"), "got: {msg}");
        assert!(msg.contains("default 30s"), "got: {msg}");
    }

    #[test]
    fn hook_timeout_key_is_rejected_regardless_of_value_shape() {
        // Any declared shape (string, zero, negative) hits the same removal
        // error rather than a type or range error for a key that no longer
        // exists.
        for value in ["\"30s\"", "0", "-5", "true"] {
            let toml = format!("[dirsql]\nhook-timeout = {value}\n");
            let err = load_config_str(&toml).unwrap_err();
            assert!(
                matches!(err, ConfigError::RemovedHookTimeout),
                "hook-timeout = {value}: got {err:?}"
            );
        }
    }

    #[test]
    fn function_parses_every_field() {
        let toml = r#"
[[dirsql.function]]
name = "embed"
args = [1, 2]
command = "dirsql-plugin-embeddings worker"
deterministic = true
timeout = "600s"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(
            config.functions,
            vec![FunctionSpec {
                name: "embed".to_string(),
                args: vec![1, 2],
                command: "dirsql-plugin-embeddings worker".to_string(),
                deterministic: true,
                timeout: Some(Duration::from_secs(600)),
            }]
        );
    }

    #[test]
    fn function_deterministic_and_timeout_default_off() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
command = "worker"
"#;
        let config = load_config_str(toml).unwrap();
        assert!(!config.functions[0].deterministic);
        assert!(config.functions[0].timeout.is_none());
    }

    #[test]
    fn functions_default_empty_when_absent() {
        let config = load_config_str("").unwrap();
        assert!(config.functions.is_empty());
    }

    #[test]
    fn multiple_functions_preserve_order() {
        let toml = r#"
[[dirsql.function]]
name = "a"
args = [1]
command = "worker-a"

[[dirsql.function]]
name = "b"
args = [1]
command = "worker-b"
"#;
        let config = load_config_str(toml).unwrap();
        let names: Vec<&str> = config.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn function_timeout_accepts_integer_seconds() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
command = "worker"
timeout = 90
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.functions[0].timeout, Some(Duration::from_secs(90)));
    }

    #[test]
    fn function_timeout_accepts_millisecond_strings() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
command = "worker"
timeout = "250ms"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(
            config.functions[0].timeout,
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn function_timeout_rejects_invalid_shapes() {
        for (value, want) in [
            ("0", "0"),
            ("-5", "-5"),
            ("\"0s\"", "\"0s\""),
            ("\"abc\"", "\"abc\""),
            ("\"5m\"", "\"5m\""),
            ("\"s\"", "\"s\""),
            ("\"-1s\"", "\"-1s\""),
        ] {
            let toml = format!(
                "[[dirsql.function]]\nname = \"f\"\nargs = [1]\ncommand = \"w\"\ntimeout = {value}\n"
            );
            let err = load_config_str(&toml).unwrap_err();
            match &err {
                ConfigError::InvalidFunctionTimeout { name, value } => {
                    assert_eq!(name, "f");
                    assert_eq!(value, want);
                }
                other => panic!("timeout = {value}: got {other:?}"),
            }
            let msg = err.to_string();
            assert!(msg.contains("600s"), "got: {msg}");
        }
    }

    #[test]
    fn function_missing_name_errors() {
        let toml = r#"
[[dirsql.function]]
args = [1]
command = "worker"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingFunctionField("name")),
            "got: {err:?}"
        );
        assert!(
            err.to_string().contains("[[dirsql.function]]"),
            "got: {err}"
        );
    }

    #[test]
    fn function_missing_command_errors() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingFunctionField("command")),
            "got: {err:?}"
        );
    }

    #[test]
    fn function_blank_command_errors() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
command = "   "
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingFunctionField("command")),
            "got: {err:?}"
        );
    }

    #[test]
    fn function_missing_args_errors() {
        let toml = r#"
[[dirsql.function]]
name = "f"
command = "worker"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingFunctionField("args")),
            "got: {err:?}"
        );
    }

    #[test]
    fn function_empty_args_list_errors() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = []
command = "worker"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(&err, ConfigError::EmptyFunctionArgs { name } if name == "f"),
            "got: {err:?}"
        );
        assert!(err.to_string().contains("args = [1]"), "got: {err}");
    }

    #[test]
    fn function_arity_out_of_range_errors() {
        for bad in ["-1", "128"] {
            let toml =
                format!("[[dirsql.function]]\nname = \"f\"\nargs = [{bad}]\ncommand = \"w\"\n");
            let err = load_config_str(&toml).unwrap_err();
            match &err {
                ConfigError::InvalidFunctionArity { name, value } => {
                    assert_eq!(name, "f");
                    assert_eq!(value.to_string(), bad);
                }
                other => panic!("args = [{bad}]: got {other:?}"),
            }
            assert!(err.to_string().contains("0..=127"), "got: {err}");
        }
    }

    #[test]
    fn function_boundary_arities_are_accepted() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [0, 127]
command = "worker"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.functions[0].args, vec![0, 127]);
    }

    #[test]
    fn function_duplicate_arity_errors() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1, 1]
command = "worker"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(
                &err,
                ConfigError::DuplicateFunctionArity { name, value: 1 } if name == "f"
            ),
            "got: {err:?}"
        );
    }

    #[test]
    fn function_invalid_name_errors() {
        for bad in ["1bad", "bad-name", "bad name", "bad.name"] {
            let toml =
                format!("[[dirsql.function]]\nname = \"{bad}\"\nargs = [1]\ncommand = \"w\"\n");
            let err = load_config_str(&toml).unwrap_err();
            assert!(
                matches!(&err, ConfigError::InvalidFunctionName { name } if name == bad),
                "name {bad}: got {err:?}"
            );
        }
    }

    #[test]
    fn function_valid_names_are_accepted() {
        for good in ["embed", "_private", "f2", "UPPER_case"] {
            let toml =
                format!("[[dirsql.function]]\nname = \"{good}\"\nargs = [1]\ncommand = \"w\"\n");
            let config = load_config_str(&toml).unwrap();
            assert_eq!(config.functions[0].name, good);
        }
    }

    #[test]
    fn function_empty_name_is_missing() {
        let toml = r#"
[[dirsql.function]]
name = ""
args = [1]
command = "worker"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(
            matches!(err, ConfigError::MissingFunctionField("name")),
            "got: {err:?}"
        );
    }

    #[test]
    fn unknown_key_in_function_is_rejected() {
        let toml = r#"
[[dirsql.function]]
name = "f"
args = [1]
command = "worker"
determinstic = true
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::Toml(_)), "got: {err:?}");
        assert!(err.to_string().contains("determinstic"), "got: {err}");
    }

    #[test]
    fn combine_concatenates_functions_in_input_order() {
        let a = cfg("[[dirsql.function]]\nname = \"fa\"\nargs = [1]\ncommand = \"wa\"\n");
        let b = cfg("[[dirsql.function]]\nname = \"fb\"\nargs = [1]\ncommand = \"wb\"\n");
        let merged = combine_configs(&[(src("/a"), a), (src("/b"), b)]).unwrap();
        let names: Vec<&str> = merged.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["fa", "fb"]);
    }

    #[test]
    fn combine_duplicate_function_name_errors_naming_both_sources() {
        let a = cfg("[[dirsql.function]]\nname = \"dup\"\nargs = [1]\ncommand = \"wa\"\n");
        let b = cfg("[[dirsql.function]]\nname = \"dup\"\nargs = [2]\ncommand = \"wb\"\n");
        let err = combine_configs(&[
            (src("/proj/.dirsql.toml"), a),
            (Source::Package("dirsql-plugin-embeddings".to_string()), b),
        ])
        .unwrap_err();
        match &err {
            ConfigError::DuplicateFunction {
                name,
                first,
                second,
            } => {
                assert_eq!(name, "dup");
                assert_eq!(first, &src("/proj/.dirsql.toml"));
                assert_eq!(
                    second,
                    &Source::Package("dirsql-plugin-embeddings".to_string())
                );
            }
            other => panic!("got: {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("'dup'"), "got: {msg}");
        assert!(msg.contains("/proj/.dirsql.toml"), "got: {msg}");
        assert!(msg.contains("dirsql-plugin-embeddings"), "got: {msg}");
    }

    #[test]
    fn combine_singleton_keeps_functions_unchanged() {
        let config = cfg("[[dirsql.function]]\nname = \"f\"\nargs = [1]\ncommand = \"w\"\n");
        let merged = combine_configs(&[(src("/a"), config.clone())]).unwrap();
        assert_eq!(merged.functions, config.functions);
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
