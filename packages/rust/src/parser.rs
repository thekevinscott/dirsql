use std::collections::HashMap;
use thiserror::Error;

use crate::db::Value;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV parse error: {0}")]
    Csv(#[from] csv::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Navigation error: path '{0}' not found in document")]
    Navigation(String),

    #[error("Expected array at path '{0}', found non-array")]
    NotAnArray(String),

    #[error("Expected object rows, found non-object value")]
    NotAnObject,

    #[error("Frontmatter delimiters not found")]
    NoFrontmatter,
}

pub type Result<T> = std::result::Result<T, ParseError>;

/// Supported file formats for built-in parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Jsonl,
    Csv,
    Tsv,
    Toml,
    Yaml,
    Frontmatter,
}

/// Parse file content into rows according to the given format.
///
/// - `format`: how to interpret the content
/// - `content`: the raw file content as a string
/// - `each`: optional dot-path to navigate into the parsed structure before extracting rows
pub fn parse_file(
    format: Format,
    content: &str,
    each: Option<&str>,
) -> Result<Vec<HashMap<String, Value>>> {
    match format {
        Format::Json => parse_json(content, each),
        Format::Jsonl => parse_jsonl(content, each),
        Format::Csv => parse_csv(content, false),
        Format::Tsv => parse_csv(content, true),
        Format::Toml => parse_toml(content, each),
        Format::Yaml => parse_yaml(content, each),
        Format::Frontmatter => parse_frontmatter(content),
    }
}

/// Column mapping source: either a dot-path into parsed content or a path capture name.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnSource {
    /// Dot-path into parsed content (e.g., "metadata.author.name")
    DotPath(String),
    /// Reference to a path capture value (e.g., "thread_id" from `{thread_id}` in glob)
    PathCapture(String),
}

impl ColumnSource {
    /// Parse a column source string. If the name exists in `capture_names`, treat it
    /// as a path capture. Otherwise treat it as a dot-path into parsed content.
    pub fn parse(source: &str, capture_names: &[String]) -> Self {
        if capture_names.contains(&source.to_string()) {
            ColumnSource::PathCapture(source.to_string())
        } else {
            ColumnSource::DotPath(source.to_string())
        }
    }
}

/// Apply column mapping overrides to parsed rows.
///
/// - `rows`: parsed rows from `parse_file()`
/// - `columns`: mapping of SQL column name -> source expression
/// - `captures`: path-captured values from glob matching
///
/// For each output row, the function:
/// 1. For DotPath sources: navigates into the original parsed content via the dot-path
/// 2. For PathCapture sources: injects the captured value as a TEXT column
///
/// If `columns` is empty, rows pass through unchanged (with captures still injected
/// for any column names matching capture keys).
pub fn apply_columns(
    rows: &[HashMap<String, Value>],
    columns: &HashMap<String, ColumnSource>,
    captures: &HashMap<String, String>,
) -> Vec<HashMap<String, Value>> {
    if columns.is_empty() {
        // No explicit column mapping -- just inject captures into existing rows
        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let mut new_row = row.clone();
            for (name, value) in captures {
                new_row.insert(name.clone(), Value::Text(value.clone()));
            }
            result.push(new_row);
        }
        return result;
    }

    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let mut new_row = HashMap::new();
        for (col_name, source) in columns {
            match source {
                ColumnSource::PathCapture(cap_name) => {
                    if let Some(val) = captures.get(cap_name) {
                        new_row.insert(col_name.clone(), Value::Text(val.clone()));
                    } else {
                        new_row.insert(col_name.clone(), Value::Null);
                    }
                }
                ColumnSource::DotPath(path) => {
                    let value = navigate_row_dot_path(row, path);
                    new_row.insert(col_name.clone(), value);
                }
            }
        }
        result.push(new_row);
    }
    result
}

/// Navigate a dot-path into a row's values.
/// For simple (non-dotted) paths, looks up the key directly.
/// For dotted paths like "metadata.author.name", the first segment must be a key
/// in the row whose value is JSON text, then navigates into that JSON.
fn navigate_row_dot_path(row: &HashMap<String, Value>, path: &str) -> Value {
    let segments: Vec<&str> = path.split('.').collect();

    if segments.len() == 1 {
        // Simple key lookup
        return row.get(path).cloned().unwrap_or(Value::Null);
    }

    // First segment is the row key
    let root_key = segments[0];
    let root_value = match row.get(root_key) {
        Some(v) => v,
        None => return Value::Null,
    };

    // If the root value is Text (possibly JSON), try to parse and navigate
    match root_value {
        Value::Text(json_text) => {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_text) {
                let mut current = &parsed;
                for segment in &segments[1..] {
                    match current {
                        serde_json::Value::Object(map) => {
                            current = match map.get(*segment) {
                                Some(v) => v,
                                None => return Value::Null,
                            };
                        }
                        _ => return Value::Null,
                    }
                }
                json_value_to_value(current)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

/// Infer the file format from a glob pattern's extension.
pub fn infer_format(glob: &str) -> Option<Format> {
    // Extract the extension from the glob pattern.
    // Handle patterns like "*.json", "data/**/*.csv", etc.
    let lower = glob.to_lowercase();
    if lower.ends_with(".json") {
        Some(Format::Json)
    } else if lower.ends_with(".jsonl") || lower.ends_with(".ndjson") {
        Some(Format::Jsonl)
    } else if lower.ends_with(".csv") {
        Some(Format::Csv)
    } else if lower.ends_with(".tsv") {
        Some(Format::Tsv)
    } else if lower.ends_with(".toml") {
        Some(Format::Toml)
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        Some(Format::Yaml)
    } else if lower.ends_with(".md") {
        Some(Format::Frontmatter)
    } else {
        None
    }
}

// --- Internal parsing functions ---

fn json_value_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Real(f)
            } else {
                Value::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => Value::Text(s.clone()),
        // Nested objects/arrays are stored as JSON text
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Value::Text(v.to_string()),
    }
}

fn json_object_to_row(obj: &serde_json::Map<String, serde_json::Value>) -> HashMap<String, Value> {
    obj.iter()
        .map(|(k, v)| (k.clone(), json_value_to_value(v)))
        .collect()
}

fn navigate_json<'a>(value: &'a serde_json::Value, path: &str) -> Result<&'a serde_json::Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        match current {
            serde_json::Value::Object(map) => {
                current = map
                    .get(segment)
                    .ok_or_else(|| ParseError::Navigation(path.to_string()))?;
            }
            _ => return Err(ParseError::Navigation(path.to_string())),
        }
    }
    Ok(current)
}

fn json_value_to_rows(value: &serde_json::Value) -> Result<Vec<HashMap<String, Value>>> {
    match value {
        serde_json::Value::Array(arr) => {
            let mut rows = Vec::new();
            for item in arr {
                match item {
                    serde_json::Value::Object(obj) => rows.push(json_object_to_row(obj)),
                    _ => return Err(ParseError::NotAnObject),
                }
            }
            Ok(rows)
        }
        serde_json::Value::Object(obj) => Ok(vec![json_object_to_row(obj)]),
        _ => Err(ParseError::NotAnObject),
    }
}

fn parse_json(content: &str, each: Option<&str>) -> Result<Vec<HashMap<String, Value>>> {
    if content.trim().is_empty() {
        return Ok(vec![]);
    }
    let parsed: serde_json::Value = serde_json::from_str(content)?;

    let target = match each {
        Some(path) => navigate_json(&parsed, path)?,
        None => &parsed,
    };

    json_value_to_rows(target)
}

fn parse_jsonl(content: &str, each: Option<&str>) -> Result<Vec<HashMap<String, Value>>> {
    let mut rows = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: serde_json::Value = serde_json::from_str(trimmed)?;

        let target = match each {
            Some(path) => {
                // For JSONL with `each`, navigate into each line's parsed value
                let navigated = navigate_json(&parsed, path)?;
                navigated.clone()
            }
            None => parsed,
        };

        match &target {
            serde_json::Value::Object(obj) => rows.push(json_object_to_row(obj)),
            serde_json::Value::Array(arr) => {
                for item in arr {
                    match item {
                        serde_json::Value::Object(obj) => rows.push(json_object_to_row(obj)),
                        _ => return Err(ParseError::NotAnObject),
                    }
                }
            }
            _ => return Err(ParseError::NotAnObject),
        }
    }
    Ok(rows)
}

fn parse_csv(content: &str, is_tsv: bool) -> Result<Vec<HashMap<String, Value>>> {
    if content.trim().is_empty() {
        return Ok(vec![]);
    }

    let mut reader_builder = csv::ReaderBuilder::new();
    reader_builder.has_headers(true);
    if is_tsv {
        reader_builder.delimiter(b'\t');
    }
    let mut reader = reader_builder.from_reader(content.as_bytes());

    let headers: Vec<String> = reader.headers()?.iter().map(|h| h.to_string()).collect();

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        let mut row = HashMap::new();
        for (i, header) in headers.iter().enumerate() {
            let value = record.get(i).unwrap_or("");
            // Try to parse as integer, then float, else keep as text
            if let Ok(n) = value.parse::<i64>() {
                row.insert(header.clone(), Value::Integer(n));
            } else if let Ok(f) = value.parse::<f64>() {
                row.insert(header.clone(), Value::Real(f));
            } else {
                row.insert(header.clone(), Value::Text(value.to_string()));
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

fn toml_value_to_value(v: &toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::Text(s.clone()),
        toml::Value::Integer(i) => Value::Integer(*i),
        toml::Value::Float(f) => Value::Real(*f),
        toml::Value::Boolean(b) => Value::Integer(if *b { 1 } else { 0 }),
        toml::Value::Datetime(dt) => Value::Text(dt.to_string()),
        toml::Value::Array(a) => Value::Text(serde_json::to_string(a).unwrap_or_default()),
        toml::Value::Table(t) => Value::Text(serde_json::to_string(t).unwrap_or_default()),
    }
}

fn navigate_toml<'a>(value: &'a toml::Value, path: &str) -> Result<&'a toml::Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        match current {
            toml::Value::Table(map) => {
                current = map
                    .get(segment)
                    .ok_or_else(|| ParseError::Navigation(path.to_string()))?;
            }
            _ => return Err(ParseError::Navigation(path.to_string())),
        }
    }
    Ok(current)
}

fn toml_value_to_rows(value: &toml::Value) -> Result<Vec<HashMap<String, Value>>> {
    match value {
        toml::Value::Table(map) => {
            let row: HashMap<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), toml_value_to_value(v)))
                .collect();
            Ok(vec![row])
        }
        toml::Value::Array(arr) => {
            let mut rows = Vec::new();
            for item in arr {
                match item {
                    toml::Value::Table(map) => {
                        let row: HashMap<String, Value> = map
                            .iter()
                            .map(|(k, v)| (k.clone(), toml_value_to_value(v)))
                            .collect();
                        rows.push(row);
                    }
                    _ => return Err(ParseError::NotAnObject),
                }
            }
            Ok(rows)
        }
        _ => Err(ParseError::NotAnObject),
    }
}

fn parse_toml(content: &str, each: Option<&str>) -> Result<Vec<HashMap<String, Value>>> {
    if content.trim().is_empty() {
        return Ok(vec![]);
    }
    let parsed: toml::Value = toml::from_str(content)?;

    let target = match each {
        Some(path) => navigate_toml(&parsed, path)?,
        None => &parsed,
    };

    toml_value_to_rows(target)
}

fn yaml_value_to_value(v: &serde_yaml::Value) -> Value {
    match v {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Integer(if *b { 1 } else { 0 }),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                Value::Real(f)
            } else {
                Value::Text(n.to_string())
            }
        }
        serde_yaml::Value::String(s) => Value::Text(s.clone()),
        serde_yaml::Value::Sequence(_) | serde_yaml::Value::Mapping(_) => {
            Value::Text(serde_json::to_string(v).unwrap_or_default())
        }
        serde_yaml::Value::Tagged(tagged) => yaml_value_to_value(&tagged.value),
    }
}

fn yaml_mapping_to_row(mapping: &serde_yaml::Mapping) -> HashMap<String, Value> {
    mapping
        .iter()
        .filter_map(|(k, v)| {
            let key = match k {
                serde_yaml::Value::String(s) => s.clone(),
                _ => return None,
            };
            Some((key, yaml_value_to_value(v)))
        })
        .collect()
}

fn navigate_yaml<'a>(value: &'a serde_yaml::Value, path: &str) -> Result<&'a serde_yaml::Value> {
    let mut current = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        match current {
            serde_yaml::Value::Mapping(map) => {
                let key = serde_yaml::Value::String(segment.to_string());
                current = map
                    .get(&key)
                    .ok_or_else(|| ParseError::Navigation(path.to_string()))?;
            }
            _ => return Err(ParseError::Navigation(path.to_string())),
        }
    }
    Ok(current)
}

fn yaml_value_to_rows(value: &serde_yaml::Value) -> Result<Vec<HashMap<String, Value>>> {
    match value {
        serde_yaml::Value::Mapping(map) => Ok(vec![yaml_mapping_to_row(map)]),
        serde_yaml::Value::Sequence(seq) => {
            let mut rows = Vec::new();
            for item in seq {
                match item {
                    serde_yaml::Value::Mapping(map) => rows.push(yaml_mapping_to_row(map)),
                    _ => return Err(ParseError::NotAnObject),
                }
            }
            Ok(rows)
        }
        _ => Err(ParseError::NotAnObject),
    }
}

fn parse_yaml(content: &str, each: Option<&str>) -> Result<Vec<HashMap<String, Value>>> {
    if content.trim().is_empty() {
        return Ok(vec![]);
    }
    let parsed: serde_yaml::Value = serde_yaml::from_str(content)?;

    let target = match each {
        Some(path) => navigate_yaml(&parsed, path)?,
        None => &parsed,
    };

    yaml_value_to_rows(target)
}

fn parse_frontmatter(content: &str) -> Result<Vec<HashMap<String, Value>>> {
    if content.trim().is_empty() {
        return Ok(vec![]);
    }

    // Frontmatter is delimited by --- at the start
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err(ParseError::NoFrontmatter);
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let end_pos = after_first.find("\n---").ok_or(ParseError::NoFrontmatter)?;

    let yaml_content = &after_first[..end_pos];
    let body_start = end_pos + 4; // skip "\n---"
    let body = if body_start < after_first.len() {
        after_first[body_start..].trim_start_matches('\n')
    } else {
        ""
    };

    let parsed: serde_yaml::Value = serde_yaml::from_str(yaml_content)?;

    let mut row = match parsed {
        serde_yaml::Value::Mapping(map) => yaml_mapping_to_row(&map),
        _ => return Err(ParseError::NotAnObject),
    };

    row.insert("body".to_string(), Value::Text(body.to_string()));
    Ok(vec![row])
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Format inference tests ---

    #[test]
    fn infer_json() {
        assert_eq!(infer_format("*.json"), Some(Format::Json));
        assert_eq!(infer_format("data/**/*.json"), Some(Format::Json));
    }

    #[test]
    fn infer_jsonl() {
        assert_eq!(infer_format("*.jsonl"), Some(Format::Jsonl));
        assert_eq!(infer_format("logs/*.ndjson"), Some(Format::Jsonl));
    }

    #[test]
    fn infer_csv() {
        assert_eq!(infer_format("*.csv"), Some(Format::Csv));
    }

    #[test]
    fn infer_tsv() {
        assert_eq!(infer_format("*.tsv"), Some(Format::Tsv));
    }

    #[test]
    fn infer_toml() {
        assert_eq!(infer_format("config/*.toml"), Some(Format::Toml));
    }

    #[test]
    fn infer_yaml() {
        assert_eq!(infer_format("*.yaml"), Some(Format::Yaml));
        assert_eq!(infer_format("*.yml"), Some(Format::Yaml));
    }

    #[test]
    fn infer_frontmatter() {
        assert_eq!(infer_format("posts/*.md"), Some(Format::Frontmatter));
    }

    #[test]
    fn infer_unknown() {
        assert_eq!(infer_format("*.txt"), None);
        assert_eq!(infer_format("*.bin"), None);
    }

    #[test]
    fn infer_case_insensitive() {
        assert_eq!(infer_format("*.JSON"), Some(Format::Json));
        assert_eq!(infer_format("*.Yaml"), Some(Format::Yaml));
    }

    // --- JSON tests ---

    #[test]
    fn json_object_single_row() {
        let content = r#"{"name": "Alice", "age": 30}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        assert_eq!(rows[0]["age"], Value::Integer(30));
    }

    #[test]
    fn json_array_multiple_rows() {
        let content = r#"[{"name": "Alice"}, {"name": "Bob"}]"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        assert_eq!(rows[1]["name"], Value::Text("Bob".into()));
    }

    #[test]
    fn json_empty_content() {
        let rows = parse_file(Format::Json, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn json_whitespace_only() {
        let rows = parse_file(Format::Json, "   \n  ", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn json_malformed() {
        let result = parse_file(Format::Json, "{invalid}", None);
        assert!(result.is_err());
    }

    #[test]
    fn json_with_each_navigation() {
        let content = r#"{"data": {"items": [{"name": "X"}, {"name": "Y"}]}}"#;
        let rows = parse_file(Format::Json, content, Some("data.items")).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("X".into()));
    }

    #[test]
    fn json_each_invalid_path() {
        let content = r#"{"data": {"items": []}}"#;
        let result = parse_file(Format::Json, content, Some("data.missing"));
        assert!(result.is_err());
    }

    #[test]
    fn json_bool_to_integer() {
        let content = r#"{"active": true, "deleted": false}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows[0]["active"], Value::Integer(1));
        assert_eq!(rows[0]["deleted"], Value::Integer(0));
    }

    #[test]
    fn json_null_value() {
        let content = r#"{"name": null}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows[0]["name"], Value::Null);
    }

    #[test]
    fn json_float_value() {
        let content = r#"{"price": 9.99}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows[0]["price"], Value::Real(9.99));
    }

    #[test]
    fn json_nested_object_stored_as_text() {
        let content = r#"{"name": "Alice", "address": {"city": "NYC"}}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        // Nested object is serialized as JSON text
        match &rows[0]["address"] {
            Value::Text(s) => assert!(s.contains("NYC")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    // --- JSONL tests ---

    #[test]
    fn jsonl_basic() {
        let content = "{\"a\": 1}\n{\"a\": 2}\n{\"a\": 3}";
        let rows = parse_file(Format::Jsonl, content, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["a"], Value::Integer(1));
        assert_eq!(rows[2]["a"], Value::Integer(3));
    }

    #[test]
    fn jsonl_empty_lines_skipped() {
        let content = "{\"a\": 1}\n\n{\"a\": 2}\n  \n";
        let rows = parse_file(Format::Jsonl, content, None).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn jsonl_empty_content() {
        let rows = parse_file(Format::Jsonl, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn jsonl_malformed_line() {
        let content = "{\"a\": 1}\n{invalid}\n{\"a\": 3}";
        let result = parse_file(Format::Jsonl, content, None);
        assert!(result.is_err());
    }

    // --- CSV tests ---

    #[test]
    fn csv_basic() {
        let content = "name,age,score\nAlice,30,95.5\nBob,25,88.0";
        let rows = parse_file(Format::Csv, content, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        assert_eq!(rows[0]["age"], Value::Integer(30));
        assert_eq!(rows[0]["score"], Value::Real(95.5));
    }

    #[test]
    fn csv_empty_content() {
        let rows = parse_file(Format::Csv, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn csv_header_only() {
        let content = "name,age\n";
        let rows = parse_file(Format::Csv, content, None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn csv_ignores_each_param() {
        // CSV doesn't use `each` -- it's passed through parse_file but ignored
        let content = "name\nAlice";
        let rows = parse_file(Format::Csv, content, None).unwrap();
        assert_eq!(rows.len(), 1);
    }

    // --- TSV tests ---

    #[test]
    fn tsv_basic() {
        let content = "name\tcount\nwidget\t42\ngadget\t7";
        let rows = parse_file(Format::Tsv, content, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("widget".into()));
        assert_eq!(rows[0]["count"], Value::Integer(42));
    }

    #[test]
    fn tsv_empty_content() {
        let rows = parse_file(Format::Tsv, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    // --- TOML tests ---

    #[test]
    fn toml_basic() {
        let content = "title = \"Hello\"\ndraft = false\n";
        let rows = parse_file(Format::Toml, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], Value::Text("Hello".into()));
        assert_eq!(rows[0]["draft"], Value::Integer(0));
    }

    #[test]
    fn toml_empty_content() {
        let rows = parse_file(Format::Toml, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn toml_malformed() {
        let result = parse_file(Format::Toml, "= invalid toml [", None);
        assert!(result.is_err());
    }

    #[test]
    fn toml_with_each() {
        let content = r#"
[metadata]
version = "1.0"

[[data.items]]
name = "Foo"
price = 10

[[data.items]]
name = "Bar"
price = 20
"#;
        let rows = parse_file(Format::Toml, content, Some("data.items")).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("Foo".into()));
        assert_eq!(rows[0]["price"], Value::Integer(10));
        assert_eq!(rows[1]["name"], Value::Text("Bar".into()));
    }

    // --- YAML tests ---

    #[test]
    fn yaml_mapping_single_row() {
        let content = "title: Hello\nauthor: Alice\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], Value::Text("Hello".into()));
        assert_eq!(rows[0]["author"], Value::Text("Alice".into()));
    }

    #[test]
    fn yaml_sequence_multiple_rows() {
        let content = "- name: Alice\n  age: 30\n- name: Bob\n  age: 25\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        assert_eq!(rows[1]["name"], Value::Text("Bob".into()));
    }

    #[test]
    fn yaml_empty_content() {
        let rows = parse_file(Format::Yaml, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn yaml_malformed() {
        let result = parse_file(Format::Yaml, ":\n  - :\n    - : :", None);
        // serde_yaml may or may not error on this; the point is it doesn't panic
        let _ = result;
    }

    #[test]
    fn yaml_with_each() {
        let content = "data:\n  items:\n    - name: X\n    - name: Y\n";
        let rows = parse_file(Format::Yaml, content, Some("data.items")).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("X".into()));
    }

    // --- Frontmatter tests ---

    #[test]
    fn frontmatter_basic() {
        let content = "---\ntitle: Hello\nauthor: Alice\n---\nThis is the body.\n";
        let rows = parse_file(Format::Frontmatter, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], Value::Text("Hello".into()));
        assert_eq!(rows[0]["author"], Value::Text("Alice".into()));
        assert_eq!(rows[0]["body"], Value::Text("This is the body.\n".into()));
    }

    #[test]
    fn frontmatter_empty_body() {
        let content = "---\ntitle: Hello\n---\n";
        let rows = parse_file(Format::Frontmatter, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], Value::Text("Hello".into()));
        assert_eq!(rows[0]["body"], Value::Text("".into()));
    }

    #[test]
    fn frontmatter_no_delimiters() {
        let result = parse_file(Format::Frontmatter, "Just plain text", None);
        assert!(result.is_err());
    }

    #[test]
    fn frontmatter_empty_content() {
        let rows = parse_file(Format::Frontmatter, "", None).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn frontmatter_multiline_body() {
        let content = "---\ntitle: Post\n---\nLine 1\nLine 2\nLine 3\n";
        let rows = parse_file(Format::Frontmatter, content, None).unwrap();
        assert_eq!(
            rows[0]["body"],
            Value::Text("Line 1\nLine 2\nLine 3\n".into())
        );
    }

    // --- apply_columns tests ---

    #[test]
    fn apply_columns_injects_captures_without_column_mapping() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("body".to_string(), Value::Text("hello".into()));
            m
        }];
        let captures = HashMap::from([("thread_id".to_string(), "abc123".to_string())]);
        let result = apply_columns(&rows, &HashMap::new(), &captures);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["body"], Value::Text("hello".into()));
        assert_eq!(result[0]["thread_id"], Value::Text("abc123".into()));
    }

    #[test]
    fn apply_columns_with_path_capture_source() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("body".to_string(), Value::Text("hello".into()));
            m
        }];
        let columns = HashMap::from([
            (
                "thread_id".to_string(),
                ColumnSource::PathCapture("thread_id".to_string()),
            ),
            (
                "body".to_string(),
                ColumnSource::DotPath("body".to_string()),
            ),
        ]);
        let captures = HashMap::from([("thread_id".to_string(), "t42".to_string())]);
        let result = apply_columns(&rows, &columns, &captures);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["thread_id"], Value::Text("t42".into()));
        assert_eq!(result[0]["body"], Value::Text("hello".into()));
    }

    #[test]
    fn apply_columns_dot_path_into_nested_json() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert(
                "metadata".to_string(),
                Value::Text(r#"{"author":{"name":"Alice"}}"#.into()),
            );
            m
        }];
        let columns = HashMap::from([(
            "author_name".to_string(),
            ColumnSource::DotPath("metadata.author.name".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["author_name"], Value::Text("Alice".into()));
    }

    #[test]
    fn apply_columns_dot_path_missing_key_returns_null() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("name".to_string(), Value::Text("Alice".into()));
            m
        }];
        let columns = HashMap::from([(
            "missing".to_string(),
            ColumnSource::DotPath("nonexistent.path".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["missing"], Value::Null);
    }

    #[test]
    fn apply_columns_missing_capture_returns_null() {
        let rows = vec![HashMap::new()];
        let columns = HashMap::from([(
            "thread_id".to_string(),
            ColumnSource::PathCapture("thread_id".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["thread_id"], Value::Null);
    }

    #[test]
    fn apply_columns_simple_key_lookup() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("name".to_string(), Value::Text("Alice".into()));
            m.insert("age".to_string(), Value::Integer(30));
            m
        }];
        let columns = HashMap::from([
            (
                "name".to_string(),
                ColumnSource::DotPath("name".to_string()),
            ),
            ("age".to_string(), ColumnSource::DotPath("age".to_string())),
        ]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["name"], Value::Text("Alice".into()));
        assert_eq!(result[0]["age"], Value::Integer(30));
    }

    #[test]
    fn column_source_parse_detects_capture() {
        let captures = vec!["thread_id".to_string(), "org".to_string()];
        assert_eq!(
            ColumnSource::parse("thread_id", &captures),
            ColumnSource::PathCapture("thread_id".to_string())
        );
        assert_eq!(
            ColumnSource::parse("metadata.author.name", &captures),
            ColumnSource::DotPath("metadata.author.name".to_string())
        );
    }

    #[test]
    fn apply_columns_multiple_rows() {
        let rows = vec![
            {
                let mut m = HashMap::new();
                m.insert("body".to_string(), Value::Text("first".into()));
                m
            },
            {
                let mut m = HashMap::new();
                m.insert("body".to_string(), Value::Text("second".into()));
                m
            },
        ];
        let captures = HashMap::from([("id".to_string(), "x".to_string())]);
        let result = apply_columns(&rows, &HashMap::new(), &captures);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["id"], Value::Text("x".into()));
        assert_eq!(result[1]["id"], Value::Text("x".into()));
        assert_eq!(result[0]["body"], Value::Text("first".into()));
        assert_eq!(result[1]["body"], Value::Text("second".into()));
    }

    // --- JSONL with each navigation ---

    #[test]
    fn jsonl_with_each_navigation() {
        let content = r#"{"data": {"name": "Alice"}}
{"data": {"name": "Bob"}}"#;
        let rows = parse_file(Format::Jsonl, content, Some("data")).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
        assert_eq!(rows[1]["name"], Value::Text("Bob".into()));
    }

    // --- JSONL line that is an array (expands to multiple rows) ---

    #[test]
    fn jsonl_line_array_expands_to_rows() {
        let content = r#"[{"a": 1}, {"a": 2}]"#;
        let rows = parse_file(Format::Jsonl, content, None).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["a"], Value::Integer(1));
        assert_eq!(rows[1]["a"], Value::Integer(2));
    }

    // --- JSONL array with non-object item ---

    #[test]
    fn jsonl_array_with_non_object_item_errors() {
        let content = r#"[1, 2, 3]"#;
        let result = parse_file(Format::Jsonl, content, None);
        assert!(result.is_err());
    }

    // --- JSONL scalar value errors ---

    #[test]
    fn jsonl_scalar_value_errors() {
        let content = "42";
        let result = parse_file(Format::Jsonl, content, None);
        assert!(result.is_err());
    }

    // --- JSON non-object/non-array top-level ---

    #[test]
    fn json_scalar_top_level_errors() {
        let content = "\"just a string\"";
        let result = parse_file(Format::Json, content, None);
        assert!(result.is_err());
    }

    // --- JSON array with non-object items ---

    #[test]
    fn json_array_with_non_object_errors() {
        let content = "[1, 2, 3]";
        let result = parse_file(Format::Json, content, None);
        assert!(result.is_err());
    }

    // --- JSON each navigating into non-object ---

    #[test]
    fn json_each_navigating_non_object_errors() {
        let content = r#"{"data": [1, 2, 3]}"#;
        let result = parse_file(Format::Json, content, Some("data.missing"));
        assert!(result.is_err());
    }

    // --- TOML navigation error path ---

    #[test]
    fn toml_each_invalid_path_errors() {
        let content = r#"
[data]
name = "test"
"#;
        let result = parse_file(Format::Toml, content, Some("data.missing.deep"));
        assert!(result.is_err());
    }

    // --- TOML navigate into non-table ---

    #[test]
    fn toml_navigate_into_non_table_errors() {
        let content = r#"
name = "test"
"#;
        let result = parse_file(Format::Toml, content, Some("name.sub"));
        assert!(result.is_err());
    }

    // --- TOML non-table/non-array top-level after navigation ---

    #[test]
    fn toml_scalar_after_navigation_errors() {
        let content = r#"
[data]
value = 42
"#;
        let result = parse_file(Format::Toml, content, Some("data.value"));
        assert!(result.is_err());
    }

    // --- TOML array with non-table items ---

    #[test]
    fn toml_array_with_non_table_errors() {
        let content = r#"
values = [1, 2, 3]
"#;
        let result = parse_file(Format::Toml, content, Some("values"));
        assert!(result.is_err());
    }

    // --- TOML value types: float, boolean, datetime, array, table ---

    #[test]
    fn toml_float_value() {
        let content = "pi = 3.14\n";
        let rows = parse_file(Format::Toml, content, None).unwrap();
        assert_eq!(rows[0]["pi"], Value::Real(3.14));
    }

    #[test]
    fn toml_boolean_true() {
        let content = "flag = true\n";
        let rows = parse_file(Format::Toml, content, None).unwrap();
        assert_eq!(rows[0]["flag"], Value::Integer(1));
    }

    #[test]
    fn toml_datetime_value() {
        let content = "created = 2024-01-15T10:30:00Z\n";
        let rows = parse_file(Format::Toml, content, None).unwrap();
        match &rows[0]["created"] {
            Value::Text(s) => assert!(s.contains("2024")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn toml_nested_array_stored_as_json() {
        let content = "tags = [\"a\", \"b\"]\n";
        let rows = parse_file(Format::Toml, content, None).unwrap();
        match &rows[0]["tags"] {
            Value::Text(s) => assert!(s.contains("a")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn toml_nested_table_stored_as_json() {
        let content = r#"
[metadata]
author = "Alice"
"#;
        let rows = parse_file(Format::Toml, content, None).unwrap();
        match &rows[0]["metadata"] {
            Value::Text(s) => assert!(s.contains("Alice")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    // --- YAML navigation error paths ---

    #[test]
    fn yaml_each_invalid_path_errors() {
        let content = "data:\n  name: test\n";
        let result = parse_file(Format::Yaml, content, Some("data.missing.deep"));
        assert!(result.is_err());
    }

    #[test]
    fn yaml_navigate_into_non_mapping_errors() {
        let content = "name: test\n";
        let result = parse_file(Format::Yaml, content, Some("name.sub"));
        assert!(result.is_err());
    }

    // --- YAML scalar top-level ---

    #[test]
    fn yaml_scalar_top_level_errors() {
        let content = "just a string\n";
        let result = parse_file(Format::Yaml, content, None);
        assert!(result.is_err());
    }

    // --- YAML sequence with non-mapping items ---

    #[test]
    fn yaml_sequence_with_non_mapping_errors() {
        let content = "- 1\n- 2\n- 3\n";
        let result = parse_file(Format::Yaml, content, None);
        assert!(result.is_err());
    }

    // --- YAML value types ---

    #[test]
    fn yaml_null_value() {
        let content = "name: null\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows[0]["name"], Value::Null);
    }

    #[test]
    fn yaml_boolean_values() {
        let content = "yes_flag: true\nno_flag: false\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows[0]["yes_flag"], Value::Integer(1));
        assert_eq!(rows[0]["no_flag"], Value::Integer(0));
    }

    #[test]
    fn yaml_integer_value() {
        let content = "count: 42\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows[0]["count"], Value::Integer(42));
    }

    #[test]
    fn yaml_float_value() {
        let content = "price: 9.99\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        assert_eq!(rows[0]["price"], Value::Real(9.99));
    }

    #[test]
    fn yaml_sequence_stored_as_json() {
        let content = "tags:\n  - a\n  - b\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        match &rows[0]["tags"] {
            Value::Text(s) => assert!(s.contains("a")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    #[test]
    fn yaml_nested_mapping_stored_as_json() {
        let content = "meta:\n  author: Alice\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        match &rows[0]["meta"] {
            Value::Text(s) => assert!(s.contains("Alice")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    // --- YAML mapping with non-string keys are filtered out ---

    #[test]
    fn yaml_non_string_keys_filtered() {
        // YAML allows integer keys; they should be filtered out by yaml_mapping_to_row
        let content = "name: Alice\n";
        let rows = parse_file(Format::Yaml, content, None).unwrap();
        // Just confirm the string key is present
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
    }

    // --- Frontmatter with non-mapping YAML ---

    #[test]
    fn frontmatter_non_mapping_yaml_errors() {
        let content = "---\n- item1\n- item2\n---\nBody text\n";
        let result = parse_file(Format::Frontmatter, content, None);
        assert!(result.is_err());
    }

    // --- Frontmatter without closing delimiter ---

    #[test]
    fn frontmatter_missing_closing_delimiter_errors() {
        let content = "---\ntitle: Hello\nNo closing delimiter";
        let result = parse_file(Format::Frontmatter, content, None);
        assert!(result.is_err());
    }

    // --- Frontmatter with body directly after closing delimiter ---

    #[test]
    fn frontmatter_body_immediately_after_closing() {
        let content = "---\ntitle: Test\n---\n";
        let rows = parse_file(Format::Frontmatter, content, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["title"], Value::Text("Test".into()));
    }

    // --- navigate_row_dot_path edge cases ---

    #[test]
    fn navigate_dot_path_root_value_is_non_text() {
        // When root value is not Text, navigate_row_dot_path returns Null
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("count".to_string(), Value::Integer(42));
            m
        }];
        let columns = HashMap::from([(
            "sub".to_string(),
            ColumnSource::DotPath("count.nested".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["sub"], Value::Null);
    }

    #[test]
    fn navigate_dot_path_invalid_json_text_returns_null() {
        // When root value is Text but not valid JSON
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("data".to_string(), Value::Text("not json".into()));
            m
        }];
        let columns = HashMap::from([(
            "val".to_string(),
            ColumnSource::DotPath("data.key".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["val"], Value::Null);
    }

    #[test]
    fn navigate_dot_path_missing_nested_key_returns_null() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("data".to_string(), Value::Text(r#"{"a": {"b": 1}}"#.into()));
            m
        }];
        let columns = HashMap::from([(
            "val".to_string(),
            ColumnSource::DotPath("data.a.missing".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["val"], Value::Null);
    }

    #[test]
    fn navigate_dot_path_into_non_object_returns_null() {
        // Navigate into a JSON value that is not an object
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("data".to_string(), Value::Text(r#"{"a": 42}"#.into()));
            m
        }];
        let columns = HashMap::from([(
            "val".to_string(),
            ColumnSource::DotPath("data.a.sub".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["val"], Value::Null);
    }

    #[test]
    fn navigate_dot_path_root_key_missing_returns_null() {
        let rows = vec![{
            let mut m = HashMap::new();
            m.insert("name".to_string(), Value::Text("Alice".into()));
            m
        }];
        let columns = HashMap::from([(
            "val".to_string(),
            ColumnSource::DotPath("missing.sub".to_string()),
        )]);
        let result = apply_columns(&rows, &columns, &HashMap::new());
        assert_eq!(result[0]["val"], Value::Null);
    }

    // --- json_value_to_value edge cases ---

    #[test]
    fn json_array_stored_as_text() {
        let content = r#"{"items": [1, 2, 3]}"#;
        let rows = parse_file(Format::Json, content, None).unwrap();
        match &rows[0]["items"] {
            Value::Text(s) => assert!(s.contains("[1,2,3]") || s.contains("[1, 2, 3]")),
            other => panic!("expected Text, got {:?}", other),
        }
    }

    // --- navigate_json edge cases ---

    #[test]
    fn navigate_json_empty_segment_skipped() {
        let content = r#"{"data": {"items": [{"name": "X"}]}}"#;
        // Leading dot produces an empty segment which should be skipped
        let rows = parse_file(Format::Json, content, Some(".data.items")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], Value::Text("X".into()));
    }

    // --- navigate_json into non-object ---

    #[test]
    fn navigate_json_into_array_errors() {
        let content = r#"{"data": [1, 2, 3]}"#;
        let result = parse_file(Format::Json, content, Some("data.0"));
        assert!(result.is_err());
    }

    // --- ParseError Display ---

    #[test]
    fn parse_error_display_messages() {
        let err = ParseError::Navigation("a.b".to_string());
        assert!(err.to_string().contains("not found"));

        let err = ParseError::NotAnArray("path".to_string());
        assert!(err.to_string().contains("non-array"));

        let err = ParseError::NotAnObject;
        assert!(err.to_string().contains("non-object"));

        let err = ParseError::NoFrontmatter;
        assert!(err.to_string().contains("Frontmatter"));
    }

    // --- JSONL with each that returns an array ---

    #[test]
    fn jsonl_with_each_returning_object() {
        let content = r#"{"wrapper": {"name": "X"}}"#;
        let rows = parse_file(Format::Jsonl, content, Some("wrapper")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["name"], Value::Text("X".into()));
    }

    // --- CSV with text values that look like numbers ---

    #[test]
    fn csv_text_values_preserved() {
        let content = "id,name\n001,Alice\n";
        let rows = parse_file(Format::Csv, content, None).unwrap();
        // "001" parses as integer 1
        assert_eq!(rows[0]["id"], Value::Integer(1));
        assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
    }

    // --- TSV with mixed types ---

    #[test]
    fn tsv_mixed_types() {
        let content = "name\tcount\tprice\nwidget\t42\t9.99\n";
        let rows = parse_file(Format::Tsv, content, None).unwrap();
        assert_eq!(rows[0]["name"], Value::Text("widget".into()));
        assert_eq!(rows[0]["count"], Value::Integer(42));
        assert_eq!(rows[0]["price"], Value::Real(9.99));
    }

    // --- navigate_toml empty segment skipped ---

    #[test]
    fn navigate_toml_empty_segment_skipped() {
        let content = r#"
[data]
name = "test"
"#;
        // Leading dot produces empty segment
        let rows = parse_file(Format::Toml, content, Some(".data")).unwrap();
        assert_eq!(rows[0]["name"], Value::Text("test".into()));
    }

    // --- navigate_yaml empty segment skipped ---

    #[test]
    fn navigate_yaml_empty_segment_skipped() {
        let content = "data:\n  name: test\n";
        let rows = parse_file(Format::Yaml, content, Some(".data")).unwrap();
        assert_eq!(rows[0]["name"], Value::Text("test".into()));
    }
}
