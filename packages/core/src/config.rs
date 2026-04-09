use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::parser::Format;

/// Error type for config loading.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("Missing required field '{0}' in [[table]] entry")]
    MissingField(&'static str),

    #[error("Unknown format '{0}'")]
    UnknownFormat(String),
}

pub type Result<T> = std::result::Result<T, ConfigError>;

/// Parsed configuration from a `.dirsql.toml` file.
#[derive(Debug, Clone)]
pub struct Config {
    pub ignore: Vec<String>,
    pub tables: Vec<TableConfig>,
}

/// Configuration for a single table.
#[derive(Debug, Clone)]
pub struct TableConfig {
    pub ddl: String,
    pub glob: String,
    pub format: Option<Format>,
    pub each: Option<String>,
    pub columns: Option<HashMap<String, String>>,
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
    ignore: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawTable {
    ddl: Option<String>,
    glob: Option<String>,
    format: Option<String>,
    each: Option<String>,
    columns: Option<HashMap<String, String>>,
    strict: Option<bool>,
}

fn parse_format_str(s: &str) -> std::result::Result<Format, ConfigError> {
    match s.to_lowercase().as_str() {
        "json" => Ok(Format::Json),
        "jsonl" | "ndjson" => Ok(Format::Jsonl),
        "csv" => Ok(Format::Csv),
        "tsv" => Ok(Format::Tsv),
        "toml" => Ok(Format::Toml),
        "yaml" | "yml" => Ok(Format::Yaml),
        "frontmatter" | "md" => Ok(Format::Frontmatter),
        other => Err(ConfigError::UnknownFormat(other.to_string())),
    }
}

/// Load and parse a `.dirsql.toml` config file from the given path.
pub fn load_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    load_config_str(&content)
}

/// Parse a `.dirsql.toml` config from a string (useful for testing).
pub fn load_config_str(content: &str) -> Result<Config> {
    let raw: RawConfig = toml::from_str(content)?;

    let ignore = raw
        .dirsql
        .and_then(|d| d.ignore)
        .unwrap_or_default();

    let raw_tables = raw.table.unwrap_or_default();
    let mut tables = Vec::with_capacity(raw_tables.len());

    for raw_table in raw_tables {
        let ddl = raw_table
            .ddl
            .ok_or(ConfigError::MissingField("ddl"))?;
        let glob = raw_table
            .glob
            .ok_or(ConfigError::MissingField("glob"))?;

        let format = match raw_table.format {
            Some(f) => Some(parse_format_str(&f)?),
            None => crate::parser::infer_format(&glob),
        };

        tables.push(TableConfig {
            ddl,
            glob,
            format,
            each: raw_table.each,
            columns: raw_table.columns,
            strict: raw_table.strict,
        });
    }

    Ok(Config { ignore, tables })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_config_parses_all_fields() {
        let toml = r#"
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT)"
glob = "_comments/{thread_id}/index.jsonl"

[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "catalog/*.json"
each = "data.items"
strict = true
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.ignore, vec!["node_modules/**", ".git/**"]);
        assert_eq!(config.tables.len(), 2);

        let t0 = &config.tables[0];
        assert_eq!(t0.ddl, "CREATE TABLE comments (thread_id TEXT, body TEXT)");
        assert_eq!(t0.glob, "_comments/{thread_id}/index.jsonl");
        assert_eq!(t0.format, Some(Format::Jsonl));
        assert!(t0.each.is_none());
        assert!(t0.strict.is_none());

        let t1 = &config.tables[1];
        assert_eq!(t1.each.as_deref(), Some("data.items"));
        assert_eq!(t1.strict, Some(true));
        assert_eq!(t1.format, Some(Format::Json));
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
    fn format_inferred_from_glob_extension() {
        let cases = vec![
            ("*.json", Some(Format::Json)),
            ("**/*.jsonl", Some(Format::Jsonl)),
            ("data/*.csv", Some(Format::Csv)),
            ("*.tsv", Some(Format::Tsv)),
            ("config/*.toml", Some(Format::Toml)),
            ("**/*.yaml", Some(Format::Yaml)),
            ("**/*.yml", Some(Format::Yaml)),
            ("**/index.md", Some(Format::Frontmatter)),
        ];

        for (glob, expected_format) in cases {
            let toml = format!(
                r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "{}"
"#,
                glob
            );
            let config = load_config_str(&toml).unwrap();
            assert_eq!(
                config.tables[0].format, expected_format,
                "format mismatch for glob: {}",
                glob
            );
        }
    }

    #[test]
    fn explicit_format_overrides_inference() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.txt"
format = "csv"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables[0].format, Some(Format::Csv));
    }

    #[test]
    fn unknown_format_returns_error() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.txt"
format = "xml"
"#;
        let err = load_config_str(toml).unwrap_err();
        assert!(err.to_string().contains("xml"));
    }

    #[test]
    fn columns_support() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (display_name TEXT)"
glob = "*.json"

[table.columns]
display_name = "metadata.author.name"
"#;
        let config = load_config_str(toml).unwrap();
        let cols = config.tables[0].columns.as_ref().unwrap();
        assert_eq!(cols.get("display_name").unwrap(), "metadata.author.name");
    }

    #[test]
    fn each_support() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "catalog/*.json"
each = "data.items"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables[0].each.as_deref(), Some("data.items"));
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
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.csv"
"#,
        )
        .unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.tables.len(), 1);
        assert_eq!(config.tables[0].format, Some(Format::Csv));
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
    fn format_string_variants() {
        // Test various format string aliases
        let cases = vec![
            ("json", Format::Json),
            ("JSON", Format::Json),
            ("jsonl", Format::Jsonl),
            ("ndjson", Format::Jsonl),
            ("csv", Format::Csv),
            ("tsv", Format::Tsv),
            ("toml", Format::Toml),
            ("yaml", Format::Yaml),
            ("yml", Format::Yaml),
            ("frontmatter", Format::Frontmatter),
            ("md", Format::Frontmatter),
        ];
        for (input, expected) in cases {
            let toml = format!(
                r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.dat"
format = "{}"
"#,
                input
            );
            let config = load_config_str(&toml).unwrap();
            assert_eq!(
                config.tables[0].format,
                Some(expected),
                "format mismatch for input: {}",
                input
            );
        }
    }

    #[test]
    fn no_format_and_unknown_extension_yields_none() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE t (x TEXT)"
glob = "*.dat"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables[0].format, None);
    }

    #[test]
    fn multiple_tables_preserve_order() {
        let toml = r#"
[[table]]
ddl = "CREATE TABLE a (x TEXT)"
glob = "a/*.json"

[[table]]
ddl = "CREATE TABLE b (x TEXT)"
glob = "b/*.csv"

[[table]]
ddl = "CREATE TABLE c (x TEXT)"
glob = "c/*.yaml"
"#;
        let config = load_config_str(toml).unwrap();
        assert_eq!(config.tables.len(), 3);
        assert!(config.tables[0].ddl.contains("a"));
        assert!(config.tables[1].ddl.contains("b"));
        assert!(config.tables[2].ddl.contains("c"));
    }
}
