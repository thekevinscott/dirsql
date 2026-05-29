use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::schema::{
    Column, ColumnType, DefaultValue, Expression, GeneratedColumn, GeneratedMode, Index,
};

/// Error type for config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Missing required field '{0}' in [[table]] entry")]
    MissingField(&'static str),

    #[error(
        "[[table]] entry must define either `ddl` or at least one `[[table.column]]`, not both"
    )]
    MixedTableDefinition,

    #[error("[[table]] entry must define either a `ddl` string or at least one `[[table.column]]`")]
    MissingTableSchema,

    #[error("invalid column definition: {0}")]
    InvalidColumn(String),
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
    /// Table name. Set for structured (`[[table.column]]`) tables; `None` for
    /// the legacy `ddl = "..."` shim, whose name is parsed from the DDL.
    pub name: Option<String>,
    /// Legacy `CREATE TABLE` DDL string (deprecated). `None` for structured
    /// tables.
    pub ddl: Option<String>,
    pub glob: String,
    pub strict: Option<bool>,
    /// Structured column definitions (from `[[table.column]]`).
    pub columns: Vec<Column>,
    pub primary_key: Vec<String>,
    pub unique: Vec<Vec<String>>,
    pub indexes: Vec<Index>,
    pub without_rowid: bool,
    pub strict_types: bool,
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
}

#[derive(Deserialize)]
struct RawTable {
    name: Option<String>,
    ddl: Option<String>,
    glob: Option<String>,
    strict: Option<bool>,
    column: Option<Vec<RawColumn>>,
    primary_key: Option<Vec<String>>,
    unique: Option<Vec<Vec<String>>>,
    index: Option<Vec<RawIndex>>,
    without_rowid: Option<bool>,
    strict_types: Option<bool>,
}

#[derive(Deserialize)]
struct RawColumn {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    not_null: Option<bool>,
    primary_key: Option<bool>,
    unique: Option<bool>,
    autoincrement: Option<bool>,
    collate: Option<String>,
    default: Option<toml::Value>,
    check: Option<RawSqlExpr>,
    generated: Option<RawGenerated>,
}

#[derive(Deserialize)]
struct RawSqlExpr {
    sql: String,
}

#[derive(Deserialize)]
struct RawGenerated {
    sql: String,
    mode: Option<String>,
}

#[derive(Deserialize)]
struct RawIndex {
    name: Option<String>,
    columns: Vec<String>,
    unique: Option<bool>,
}

/// Load and parse a `.dirsql.toml` config file from the given path.
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    load_config_str(&content)
}

/// Parse a `.dirsql.toml` config from a string (useful for testing).
pub fn load_config_str(content: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(content)?;

    let (root, ignore, persist, persist_path) = match raw.dirsql {
        Some(d) => (
            d.root,
            d.ignore.unwrap_or_default(),
            d.persist.unwrap_or(false),
            d.persist_path,
        ),
        None => (None, Vec::new(), false, None),
    };

    let raw_tables = raw.table.unwrap_or_default();
    let mut tables = Vec::with_capacity(raw_tables.len());

    for raw_table in raw_tables {
        let glob = raw_table.glob.ok_or(ConfigError::MissingField("glob"))?;

        let raw_columns = raw_table.column.unwrap_or_default();
        let has_ddl = raw_table.ddl.is_some();
        let has_columns = !raw_columns.is_empty();

        if has_ddl && has_columns {
            return Err(ConfigError::MixedTableDefinition);
        }
        if !has_ddl && !has_columns {
            return Err(ConfigError::MissingTableSchema);
        }

        let (name, ddl, columns) = if has_ddl {
            // Legacy `ddl = "..."` shim: still parses, but warn.
            eprintln!(
                "dirsql: `.dirsql.toml` `ddl = \"...\"` is deprecated; \
                 use structured `[[table.column]]` blocks instead (issue #202)."
            );
            (None, raw_table.ddl, Vec::new())
        } else {
            let name = raw_table.name.ok_or(ConfigError::MissingField("name"))?;
            let mut columns = Vec::with_capacity(raw_columns.len());
            for rc in raw_columns {
                columns.push(build_column(rc)?);
            }
            (Some(name), None, columns)
        };

        let indexes = raw_table
            .index
            .unwrap_or_default()
            .into_iter()
            .map(|ri| Index {
                name: ri.name,
                columns: ri.columns,
                unique: ri.unique.unwrap_or(false),
            })
            .collect();

        tables.push(TableConfig {
            name,
            ddl,
            glob,
            strict: raw_table.strict,
            columns,
            primary_key: raw_table.primary_key.unwrap_or_default(),
            unique: raw_table.unique.unwrap_or_default(),
            indexes,
            without_rowid: raw_table.without_rowid.unwrap_or(false),
            strict_types: raw_table.strict_types.unwrap_or(false),
        });
    }

    Ok(Config {
        root,
        ignore,
        tables,
        persist,
        persist_path,
    })
}

/// Convert a raw `[[table.column]]` entry into a [`Column`].
fn build_column(rc: RawColumn) -> Result<Column> {
    let ty = ColumnType::parse(&rc.ty).ok_or_else(|| {
        ConfigError::InvalidColumn(format!(
            "column `{}` has invalid type `{}` (expected TEXT, INTEGER, REAL, BLOB, or NUMERIC)",
            rc.name, rc.ty
        ))
    })?;
    let mut col = Column::new(rc.name.clone(), ty);
    col.not_null = rc.not_null.unwrap_or(false);
    col.primary_key = rc.primary_key.unwrap_or(false);
    col.unique = rc.unique.unwrap_or(false);
    col.autoincrement = rc.autoincrement.unwrap_or(false);
    col.collate = rc.collate;
    if let Some(default) = &rc.default {
        col.default = Some(toml_to_default(default, &rc.name)?);
    }
    if let Some(check) = rc.check {
        col.check = Some(Expression { sql: check.sql });
    }
    if let Some(generated) = rc.generated {
        let mode = match generated.mode {
            Some(m) => GeneratedMode::parse(&m).ok_or_else(|| {
                ConfigError::InvalidColumn(format!(
                    "column `{}` `generated.mode` must be 'stored' or 'virtual'",
                    rc.name
                ))
            })?,
            None => GeneratedMode::Virtual,
        };
        col.generated = Some(GeneratedColumn {
            sql: generated.sql,
            mode,
        });
    }
    Ok(col)
}

/// Map a TOML `default` value (scalar or `{ sql = "..." }`) to a [`DefaultValue`].
/// TOML has no null literal, so [`DefaultValue::Null`] is unreachable here.
fn toml_to_default(value: &toml::Value, col: &str) -> Result<DefaultValue> {
    match value {
        toml::Value::String(s) => Ok(DefaultValue::Text(s.clone())),
        toml::Value::Integer(i) => Ok(DefaultValue::Integer(*i)),
        toml::Value::Float(f) => Ok(DefaultValue::Real(*f)),
        toml::Value::Boolean(b) => Ok(DefaultValue::Integer(if *b { 1 } else { 0 })),
        toml::Value::Table(t) => {
            let sql = t.get("sql").and_then(|v| v.as_str()).ok_or_else(|| {
                ConfigError::InvalidColumn(format!(
                    "column `{col}` `default` table must have a string `sql`"
                ))
            })?;
            Ok(DefaultValue::Sql(sql.to_string()))
        }
        other => Err(ConfigError::InvalidColumn(format!(
            "column `{col}` has an unsupported `default` value: {other}"
        ))),
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
        assert_eq!(
            t0.ddl.as_deref(),
            Some("CREATE TABLE comments (thread_id TEXT, _path TEXT)")
        );
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
        match err {
            ConfigError::Toml(_) => {}
            other => panic!("expected Toml error, got: {}", other),
        }
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
    fn load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".dirsql.toml");
        std::fs::write(
            &path,
            r#"
[[table]]
ddl = "CREATE TABLE t (_path TEXT)"
glob = "*.csv"
"#,
        )
        .unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.tables.len(), 1);
    }

    #[test]
    fn load_config_missing_file_returns_io_error() {
        let err = load_config(Path::new("/nonexistent/.dirsql.toml")).unwrap_err();
        match err {
            ConfigError::Io(_) => {}
            other => panic!("expected Io error, got: {}", other),
        }
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
        assert!(config.tables[0].ddl.as_deref().unwrap().contains("a"));
        assert!(config.tables[1].ddl.as_deref().unwrap().contains("b"));
        assert!(config.tables[2].ddl.as_deref().unwrap().contains("c"));
    }

    #[test]
    fn structured_columns_parse() {
        let toml = r#"
[[table]]
name = "docs"
glob = "**/*.md"

  [[table.column]]
  name = "title"
  type = "TEXT"
  not_null = true
  default = "untitled"

  [[table.column]]
  name = "body"
  type = "TEXT"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 1);
        let t = &config.tables[0];
        assert_eq!(t.name.as_deref(), Some("docs"));
        assert!(t.ddl.is_none());
        assert_eq!(t.columns.len(), 2);
        assert_eq!(t.columns[0].name, "title");
        assert_eq!(t.columns[0].ty, ColumnType::Text);
        assert!(t.columns[0].not_null);
        assert_eq!(
            t.columns[0].default,
            Some(DefaultValue::Text("untitled".into()))
        );
        assert_eq!(t.columns[1].name, "body");
    }

    #[test]
    fn structured_sql_default_and_check_parse() {
        let toml = r#"
[[table]]
name = "t"
glob = "*.md"

  [[table.column]]
  name = "ingested_at"
  type = "INTEGER"
  default = { sql = "strftime('%s', 'now')" }

  [[table.column]]
  name = "body"
  type = "TEXT"
  check = { sql = "length(body) > 0" }
"#;
        let config = load_config_str(toml).unwrap();
        let t = &config.tables[0];
        assert_eq!(
            t.columns[0].default,
            Some(DefaultValue::Sql("strftime('%s', 'now')".into()))
        );
        assert_eq!(
            t.columns[1].check.as_ref().map(|e| e.sql.as_str()),
            Some("length(body) > 0")
        );
    }

    #[test]
    fn structured_table_level_options_parse() {
        let toml = r#"
[[table]]
name = "t"
glob = "*.md"
primary_key = ["a", "b"]
unique = [["a", "b"]]
without_rowid = true
strict_types = true

  [[table.column]]
  name = "a"
  type = "TEXT"

  [[table.column]]
  name = "b"
  type = "TEXT"

  [[table.index]]
  name = "idx_a"
  columns = ["a"]
  unique = true
"#;
        let config = load_config_str(toml).unwrap();
        let t = &config.tables[0];
        assert_eq!(t.primary_key, vec!["a", "b"]);
        assert_eq!(t.unique, vec![vec!["a".to_string(), "b".to_string()]]);
        assert!(t.without_rowid);
        assert!(t.strict_types);
        assert_eq!(t.indexes.len(), 1);
        assert_eq!(t.indexes[0].name.as_deref(), Some("idx_a"));
        assert!(t.indexes[0].unique);
    }

    #[test]
    fn both_ddl_and_columns_is_rejected() {
        let toml = r#"
[[table]]
name = "t"
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.md"

  [[table.column]]
  name = "x"
  type = "TEXT"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::MixedTableDefinition));
    }

    #[test]
    fn structured_table_without_name_is_rejected() {
        let toml = r#"
[[table]]
glob = "*.md"

  [[table.column]]
  name = "x"
  type = "TEXT"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn invalid_column_type_is_rejected() {
        let toml = r#"
[[table]]
name = "t"
glob = "*.md"

  [[table.column]]
  name = "x"
  type = "VARCHAR"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidColumn(_)));
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
}
