//! LLM-assisted schema inference for `dirsql init` (issue #96).
//!
//! This module is a pure-Rust framework: it walks a directory, builds a
//! prompt for an LLM, parses the LLM's JSON response, and renders the
//! result as `.dirsql.toml`. The actual LLM call is **out of scope** for
//! this module — it is the CLI's job (or a future `Inferer` trait
//! implementation) to make the network call and feed the response back in
//! via `parse_response`. Splitting the framework from the network call
//! keeps the entire pipeline testable without LLM credentials.
//!
//! See `docs/guide/init.md` for the user-facing surface.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid LLM response JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("LLM response missing required field '{0}'")]
    MissingField(&'static str),

    #[error("LLM response has empty `tables` array; nothing to write")]
    EmptyTables,

    #[error("output file already exists at {0} (pass --force to overwrite)")]
    OutputExists(PathBuf),
}

pub type Result<T> = std::result::Result<T, InferenceError>;

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

/// Knobs for [`sample_directory`]. The defaults bound the prompt size at
/// roughly tens of kilobytes, which is well below any current LLM context
/// limit while still giving the model enough signal to propose tables.
#[derive(Debug, Clone)]
pub struct SampleOptions {
    /// Hard cap on files visited during the walk. Files beyond this are
    /// counted (per pattern) but not opened or sampled.
    pub max_files: usize,
    /// Max bytes of content read per sampled file.
    pub max_sample_bytes: usize,
    /// Max files to *content-sample* per inferred glob pattern. Other
    /// matching files contribute to the `file_count` only.
    pub max_samples_per_pattern: usize,
    /// Patterns to skip during the walk. Behaves like `.gitignore`-style
    /// "directory or file path contains this segment". This is
    /// deliberately coarse — the LLM will refine ignore rules in its
    /// response.
    pub ignore_segments: Vec<String>,
}

impl Default for SampleOptions {
    fn default() -> Self {
        Self {
            max_files: 200,
            max_sample_bytes: 512,
            max_samples_per_pattern: 3,
            ignore_segments: vec![
                ".git".into(),
                ".dirsql".into(),
                "node_modules".into(),
                "target".into(),
                ".venv".into(),
                "__pycache__".into(),
            ],
        }
    }
}

/// Snapshot of a directory grouped by inferred glob pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectorySummary {
    pub root: PathBuf,
    pub patterns: Vec<PatternSample>,
    /// `true` when the walk was truncated by `max_files`. Surfaced in the
    /// prompt so the LLM knows the picture is incomplete.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSample {
    /// Inferred glob (e.g. `posts/*.json`, `**/*.md`). Always uses `/`.
    pub glob: String,
    /// How many files matched this pattern in the walk.
    pub file_count: usize,
    /// Up to `max_samples_per_pattern` content samples.
    pub samples: Vec<FileSample>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSample {
    pub rel_path: String,
    pub size: u64,
    pub content_preview: String,
    /// `true` when the file was longer than `max_sample_bytes` and the
    /// preview was truncated.
    pub truncated: bool,
}

/// Walk `root`, group files by their parent-directory + extension, and
/// return a per-pattern summary. Symlinks are not followed; hidden dirs
/// matching `ignore_segments` are skipped.
pub fn sample_directory(root: &Path, opts: &SampleOptions) -> Result<DirectorySummary> {
    // Group: glob string -> (count, Vec<(rel_path, size)>)
    let mut groups: BTreeMap<String, (usize, Vec<(String, u64)>)> = BTreeMap::new();
    let mut visited = 0usize;
    let mut truncated = false;

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_ignored_segment(e.path(), &opts.ignore_segments, root))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if visited >= opts.max_files {
            truncated = true;
            break;
        }
        visited += 1;

        let rel = match entry.path().strip_prefix(root) {
            Ok(p) => p.to_path_buf(),
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let glob = infer_glob_for_path(&rel);
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

        let entry = groups.entry(glob).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < opts.max_samples_per_pattern {
            entry.1.push((rel_str, size));
        }
    }

    let mut patterns = Vec::with_capacity(groups.len());
    for (glob, (count, picks)) in groups {
        let mut samples = Vec::with_capacity(picks.len());
        for (rel_str, size) in picks {
            let abs = root.join(&rel_str);
            let (preview, was_truncated) = read_preview(&abs, opts.max_sample_bytes)?;
            samples.push(FileSample {
                rel_path: rel_str,
                size,
                content_preview: preview,
                truncated: was_truncated,
            });
        }
        patterns.push(PatternSample {
            glob,
            file_count: count,
            samples,
        });
    }

    Ok(DirectorySummary {
        root: root.to_path_buf(),
        patterns,
        truncated,
    })
}

fn is_ignored_segment(path: &Path, segments: &[String], root: &Path) -> bool {
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    segments.iter().any(|seg| seg == name)
}

/// Map a relative file path to an inferred glob. Files inside a directory
/// collapse to `<dir>/*.<ext>`; root-level files collapse to `*.<ext>`.
/// Files without an extension keep their literal name.
fn infer_glob_for_path(rel: &Path) -> String {
    let parent = rel.parent().filter(|p| !p.as_os_str().is_empty());
    let ext = rel.extension().and_then(|e| e.to_str());
    match (parent, ext) {
        (Some(p), Some(ext)) => format!("{}/*.{ext}", p.to_string_lossy().replace('\\', "/")),
        (None, Some(ext)) => format!("*.{ext}"),
        (Some(p), None) => format!(
            "{}/{}",
            p.to_string_lossy().replace('\\', "/"),
            rel.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        ),
        (None, None) => rel.to_string_lossy().to_string(),
    }
}

fn read_preview(path: &Path, max_bytes: usize) -> Result<(String, bool)> {
    let metadata = fs::metadata(path)?;
    let size = metadata.len() as usize;
    let bytes = fs::read(path)?;
    let truncated = size > max_bytes;
    let slice = &bytes[..bytes.len().min(max_bytes)];
    // Lossy decode; binary noise is fine — the LLM will see "bytes
    // unreadable as UTF-8" and propose a different format or skip.
    Ok((String::from_utf8_lossy(slice).to_string(), truncated))
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

/// Render the directory summary into a single text blob that an LLM can
/// turn into a `.dirsql.toml`. Format is intentionally simple: a header
/// describing the JSON contract, followed by one block per pattern.
pub fn build_prompt(summary: &DirectorySummary) -> String {
    let mut out = String::new();
    out.push_str(PROMPT_PREAMBLE);
    out.push_str("\n\n");
    out.push_str(&format!("Root: {}\n", summary.root.display()));
    if summary.truncated {
        out.push_str(
            "Note: directory walk was truncated; the LLM should propose\n\
             ignore patterns that match the dominant non-data directories.\n",
        );
    }
    out.push_str("Patterns observed (count = files matched by walk):\n\n");
    for pat in &summary.patterns {
        out.push_str(&format!(
            "- glob: `{}` (count={})\n",
            pat.glob, pat.file_count
        ));
        for sample in &pat.samples {
            out.push_str(&format!(
                "  - sample: `{}` ({} bytes",
                sample.rel_path, sample.size
            ));
            if sample.truncated {
                out.push_str(", preview truncated");
            }
            out.push_str(")\n");
            out.push_str("    ```\n");
            for line in sample.content_preview.lines().take(40) {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("    ```\n");
        }
    }
    out
}

const PROMPT_PREAMBLE: &str = "\
You are configuring `dirsql`, a tool that exposes a directory as a SQLite
database. Inspect the directory summary below and propose a `.dirsql.toml`
configuration as JSON, matching this schema:

  {
    \"ignore\": [\"<glob>\", ...],
    \"tables\": [
      {
        \"ddl\": \"CREATE TABLE name (col TYPE, ...)\",
        \"glob\": \"<relative glob>\",
        \"format\": \"json|jsonl|csv|tsv|toml|yaml|frontmatter\" | null,
        \"each\": \"<dot.path>\" | null,
        \"columns\": { \"col\": \"<source>\", ... } | null
      },
      ...
    ]
  }

Reply with ONLY the JSON object — no prose, no Markdown fences. Each glob
is relative to the directory root. Pick concise, snake_case table names.";

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredConfig {
    pub ignore: Vec<String>,
    pub tables: Vec<InferredTable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredTable {
    pub ddl: String,
    pub glob: String,
    pub format: Option<String>,
    pub each: Option<String>,
    pub columns: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct RawResponse {
    #[serde(default)]
    ignore: Option<Vec<String>>,
    tables: Option<Vec<RawTable>>,
}

#[derive(Deserialize)]
struct RawTable {
    ddl: Option<String>,
    glob: Option<String>,
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    each: Option<String>,
    #[serde(default)]
    columns: Option<HashMap<String, String>>,
}

/// Parse a JSON response into a validated [`InferredConfig`].
///
/// Tolerates a leading Markdown code fence (```json ... ```) if the LLM
/// produces one despite instructions; otherwise the JSON must be the
/// entire payload.
pub fn parse_response(json: &str) -> Result<InferredConfig> {
    let cleaned = strip_code_fence(json.trim());
    let raw: RawResponse = serde_json::from_str(cleaned)?;
    let raw_tables = raw.tables.ok_or(InferenceError::MissingField("tables"))?;
    if raw_tables.is_empty() {
        return Err(InferenceError::EmptyTables);
    }
    let mut tables = Vec::with_capacity(raw_tables.len());
    for raw in raw_tables {
        let ddl = raw
            .ddl
            .ok_or(InferenceError::MissingField("tables[].ddl"))?;
        let glob = raw
            .glob
            .ok_or(InferenceError::MissingField("tables[].glob"))?;
        tables.push(InferredTable {
            ddl,
            glob,
            format: raw.format,
            each: raw.each,
            columns: raw.columns,
        });
    }
    Ok(InferredConfig {
        ignore: raw.ignore.unwrap_or_default(),
        tables,
    })
}

fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json").or_else(|| s.strip_prefix("```")) {
        let rest = rest.trim_start_matches('\n');
        if let Some(end) = rest.rfind("```") {
            return rest[..end].trim();
        }
    }
    s
}

// ---------------------------------------------------------------------------
// TOML rendering
// ---------------------------------------------------------------------------

/// Render an [`InferredConfig`] as a `.dirsql.toml` document.
pub fn render_toml(config: &InferredConfig) -> String {
    let mut out = String::new();
    out.push_str("# Generated by `dirsql init`. Edit freely.\n\n");
    if !config.ignore.is_empty() {
        out.push_str("[dirsql]\n");
        out.push_str("ignore = [");
        for (i, pat) in config.ignore.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&toml_string_lit(pat));
        }
        out.push_str("]\n\n");
    }
    for table in &config.tables {
        out.push_str("[[table]]\n");
        out.push_str(&format!("ddl = {}\n", toml_string_lit(&table.ddl)));
        out.push_str(&format!("glob = {}\n", toml_string_lit(&table.glob)));
        if let Some(fmt) = &table.format {
            out.push_str(&format!("format = {}\n", toml_string_lit(fmt)));
        }
        if let Some(each) = &table.each {
            out.push_str(&format!("each = {}\n", toml_string_lit(each)));
        }
        if let Some(columns) = &table.columns
            && !columns.is_empty()
        {
            out.push_str("\n[table.columns]\n");
            let mut keys: Vec<&String> = columns.keys().collect();
            keys.sort();
            for k in keys {
                let v = &columns[k];
                out.push_str(&format!("{k} = {}\n", toml_string_lit(v)));
            }
        }
        out.push('\n');
    }
    out
}

fn toml_string_lit(s: &str) -> String {
    // Use a basic-string with escaped control chars + quotes; the values
    // we render (DDL, globs) are short single-line strings.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Template mode (no LLM)
// ---------------------------------------------------------------------------

/// Generate a starter [`InferredConfig`] from a [`DirectorySummary`] using
/// purely heuristic rules: each pattern with a recognised file extension
/// becomes its own table. Columns are stubbed as `payload TEXT`; the user
/// is expected to refine the DDL by hand. This is the default behavior
/// of `dirsql init` (without `--infer`).
pub fn template_from_summary(summary: &DirectorySummary) -> InferredConfig {
    let mut tables = Vec::new();
    let mut seen_names: HashMap<String, usize> = HashMap::new();
    for pat in &summary.patterns {
        let Some(ext) = extension_of_glob(&pat.glob) else {
            continue;
        };
        if !is_known_extension(ext) {
            continue;
        }
        let base = table_name_for_pattern(&pat.glob, ext);
        let name = match seen_names.get(&base).copied() {
            None => {
                seen_names.insert(base.clone(), 1);
                base
            }
            Some(n) => {
                seen_names.insert(base.clone(), n + 1);
                format!("{base}_{n}")
            }
        };
        tables.push(InferredTable {
            ddl: format!("CREATE TABLE {name} (payload TEXT)"),
            glob: pat.glob.clone(),
            format: None,
            each: None,
            columns: None,
        });
    }
    InferredConfig {
        ignore: vec![
            "node_modules/**".into(),
            ".git/**".into(),
            "target/**".into(),
        ],
        tables,
    }
}

fn extension_of_glob(glob: &str) -> Option<&str> {
    let dot = glob.rfind('.')?;
    let rest = &glob[dot + 1..];
    if rest.is_empty() || rest.contains('/') {
        None
    } else {
        Some(rest)
    }
}

fn is_known_extension(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "json" | "jsonl" | "ndjson" | "csv" | "tsv" | "toml" | "yaml" | "yml" | "md"
    )
}

fn table_name_for_pattern(glob: &str, ext: &str) -> String {
    // Use the parent dir as the base name when present; otherwise fall
    // back to the extension.
    let trimmed = glob.trim_end_matches(&format!("/*.{ext}"));
    if trimmed == glob || trimmed.is_empty() {
        sanitize_ident(ext)
    } else {
        sanitize_ident(trimmed.split('/').next_back().unwrap_or(ext))
    }
}

fn sanitize_ident(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "items".into()
    } else if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("t_{trimmed}")
    } else {
        trimmed
    }
}

// ---------------------------------------------------------------------------
// File output
// ---------------------------------------------------------------------------

/// Write `toml` to `path`. Refuses to overwrite an existing file unless
/// `force` is true.
pub fn write_config(path: &Path, toml: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(InferenceError::OutputExists(path.to_path_buf()));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml)?;
    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn writefile(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    // ---- sample_directory --------------------------------------------------

    #[test]
    fn sample_directory_groups_files_by_inferred_glob() {
        let tmp = TempDir::new().unwrap();
        writefile(tmp.path(), "posts/a.json", r#"{"title":"a"}"#);
        writefile(tmp.path(), "posts/b.json", r#"{"title":"b"}"#);
        writefile(tmp.path(), "data/m.csv", "name,age\nalice,30\n");

        let summary = sample_directory(tmp.path(), &SampleOptions::default()).unwrap();
        let globs: Vec<&str> = summary.patterns.iter().map(|p| p.glob.as_str()).collect();
        assert!(globs.contains(&"posts/*.json"), "globs={globs:?}");
        assert!(globs.contains(&"data/*.csv"), "globs={globs:?}");

        let posts = summary
            .patterns
            .iter()
            .find(|p| p.glob == "posts/*.json")
            .unwrap();
        assert_eq!(posts.file_count, 2);
        assert!(!posts.samples.is_empty());
    }

    #[test]
    fn sample_directory_skips_ignored_segments() {
        let tmp = TempDir::new().unwrap();
        writefile(tmp.path(), "posts/a.json", "{}");
        writefile(tmp.path(), "node_modules/lib/x.json", "{}");
        writefile(tmp.path(), ".git/config", "");

        let summary = sample_directory(tmp.path(), &SampleOptions::default()).unwrap();
        let globs: Vec<&str> = summary.patterns.iter().map(|p| p.glob.as_str()).collect();
        assert!(globs.iter().all(|g| !g.contains("node_modules")));
        assert!(globs.iter().all(|g| !g.contains(".git")));
    }

    #[test]
    fn sample_directory_caps_samples_per_pattern() {
        let tmp = TempDir::new().unwrap();
        for i in 0..10 {
            writefile(tmp.path(), &format!("posts/{i}.json"), "{}");
        }
        let opts = SampleOptions {
            max_samples_per_pattern: 2,
            ..SampleOptions::default()
        };
        let summary = sample_directory(tmp.path(), &opts).unwrap();
        let posts = &summary.patterns[0];
        assert_eq!(posts.file_count, 10);
        assert_eq!(posts.samples.len(), 2);
    }

    #[test]
    fn sample_directory_truncates_long_file_preview() {
        let tmp = TempDir::new().unwrap();
        let big = "x".repeat(2048);
        writefile(tmp.path(), "data/big.json", &big);
        let opts = SampleOptions {
            max_sample_bytes: 16,
            ..SampleOptions::default()
        };
        let summary = sample_directory(tmp.path(), &opts).unwrap();
        let s = &summary.patterns[0].samples[0];
        assert!(s.truncated);
        assert_eq!(s.content_preview.len(), 16);
    }

    #[test]
    fn sample_directory_marks_walk_truncation() {
        let tmp = TempDir::new().unwrap();
        for i in 0..50 {
            writefile(tmp.path(), &format!("data/{i}.json"), "{}");
        }
        let opts = SampleOptions {
            max_files: 10,
            ..SampleOptions::default()
        };
        let summary = sample_directory(tmp.path(), &opts).unwrap();
        assert!(summary.truncated);
    }

    // ---- build_prompt ------------------------------------------------------

    #[test]
    fn build_prompt_includes_globs_and_samples() {
        let tmp = TempDir::new().unwrap();
        writefile(tmp.path(), "posts/a.json", r#"{"title":"hello"}"#);
        let summary = sample_directory(tmp.path(), &SampleOptions::default()).unwrap();
        let prompt = build_prompt(&summary);
        assert!(prompt.contains("posts/*.json"));
        assert!(prompt.contains(r#"{"title":"hello"}"#));
        assert!(prompt.contains("CREATE TABLE")); // schema hint
    }

    #[test]
    fn build_prompt_notes_walk_truncation() {
        let summary = DirectorySummary {
            root: PathBuf::from("/tmp/x"),
            patterns: vec![],
            truncated: true,
        };
        let prompt = build_prompt(&summary);
        assert!(prompt.contains("truncated"));
    }

    // ---- parse_response ----------------------------------------------------

    #[test]
    fn parse_response_accepts_minimal_object() {
        let json = r#"{
          "tables": [
            {"ddl": "CREATE TABLE posts (title TEXT)", "glob": "posts/*.json"}
          ]
        }"#;
        let cfg = parse_response(json).unwrap();
        assert_eq!(cfg.tables.len(), 1);
        assert_eq!(cfg.tables[0].glob, "posts/*.json");
        assert!(cfg.ignore.is_empty());
    }

    #[test]
    fn parse_response_strips_code_fence() {
        let json = "```json\n{\"tables\":[{\"ddl\":\"CREATE TABLE p (t TEXT)\",\"glob\":\"*.json\"}]}\n```";
        let cfg = parse_response(json).unwrap();
        assert_eq!(cfg.tables.len(), 1);
    }

    #[test]
    fn parse_response_errors_on_missing_tables() {
        let json = r#"{"ignore": []}"#;
        let err = parse_response(json).unwrap_err();
        assert!(matches!(err, InferenceError::MissingField("tables")));
    }

    #[test]
    fn parse_response_errors_on_empty_tables() {
        let json = r#"{"tables": []}"#;
        let err = parse_response(json).unwrap_err();
        assert!(matches!(err, InferenceError::EmptyTables));
    }

    #[test]
    fn parse_response_errors_on_table_missing_ddl() {
        let json = r#"{"tables": [{"glob": "*.json"}]}"#;
        let err = parse_response(json).unwrap_err();
        assert!(matches!(err, InferenceError::MissingField("tables[].ddl")));
    }

    #[test]
    fn parse_response_preserves_optional_fields() {
        let json = r#"{
          "ignore": ["node_modules/**"],
          "tables": [{
            "ddl": "CREATE TABLE c (thread_id TEXT, body TEXT)",
            "glob": "_comments/{thread_id}/index.jsonl",
            "format": "jsonl",
            "each": "data.items",
            "columns": {"thread_id": "thread_id"}
          }]
        }"#;
        let cfg = parse_response(json).unwrap();
        assert_eq!(cfg.ignore, vec!["node_modules/**".to_string()]);
        let t = &cfg.tables[0];
        assert_eq!(t.format.as_deref(), Some("jsonl"));
        assert_eq!(t.each.as_deref(), Some("data.items"));
        assert_eq!(
            t.columns
                .as_ref()
                .unwrap()
                .get("thread_id")
                .map(String::as_str),
            Some("thread_id")
        );
    }

    // ---- render_toml -------------------------------------------------------

    #[test]
    fn render_toml_round_trips_through_load_config() {
        let cfg = InferredConfig {
            ignore: vec!["node_modules/**".into()],
            tables: vec![InferredTable {
                ddl: "CREATE TABLE posts (title TEXT, author TEXT)".into(),
                glob: "posts/*.json".into(),
                format: Some("json".into()),
                each: None,
                columns: None,
            }],
        };
        let toml = render_toml(&cfg);
        let parsed = crate::config::load_config_str(&toml).expect("rendered toml should parse");
        assert_eq!(parsed.tables.len(), 1);
        assert_eq!(parsed.tables[0].glob, "posts/*.json");
        assert_eq!(parsed.ignore, vec!["node_modules/**".to_string()]);
    }

    #[test]
    fn render_toml_includes_each_and_columns() {
        let mut cols = HashMap::new();
        cols.insert("thread_id".to_string(), "thread_id".to_string());
        let cfg = InferredConfig {
            ignore: vec![],
            tables: vec![InferredTable {
                ddl: "CREATE TABLE c (thread_id TEXT)".into(),
                glob: "_comments/{thread_id}/index.jsonl".into(),
                format: Some("jsonl".into()),
                each: Some("data.items".into()),
                columns: Some(cols),
            }],
        };
        let toml = render_toml(&cfg);
        assert!(toml.contains("each = \"data.items\""), "{toml}");
        assert!(toml.contains("[table.columns]"), "{toml}");
        assert!(toml.contains("thread_id = \"thread_id\""), "{toml}");
        // Confirm it still parses.
        crate::config::load_config_str(&toml).unwrap();
    }

    #[test]
    fn render_toml_escapes_backslashes_and_quotes() {
        let cfg = InferredConfig {
            ignore: vec![],
            tables: vec![InferredTable {
                ddl: "CREATE TABLE q (msg TEXT)".into(),
                glob: r#"weird\\"name/*.json"#.into(),
                format: None,
                each: None,
                columns: None,
            }],
        };
        let toml = render_toml(&cfg);
        // The rendered string must round-trip through TOML parsing.
        crate::config::load_config_str(&toml).unwrap();
    }

    // ---- template_from_summary --------------------------------------------

    #[test]
    fn template_creates_one_table_per_known_extension() {
        let tmp = TempDir::new().unwrap();
        writefile(tmp.path(), "posts/a.json", "{}");
        writefile(tmp.path(), "data/m.csv", "x\n1\n");
        writefile(tmp.path(), "notes/n.md", "---\ntitle: a\n---\nbody");
        writefile(tmp.path(), "binary/b.dat", "?");

        let summary = sample_directory(tmp.path(), &SampleOptions::default()).unwrap();
        let cfg = template_from_summary(&summary);

        let globs: Vec<&str> = cfg.tables.iter().map(|t| t.glob.as_str()).collect();
        assert!(globs.contains(&"posts/*.json"), "{globs:?}");
        assert!(globs.contains(&"data/*.csv"), "{globs:?}");
        assert!(globs.contains(&"notes/*.md"), "{globs:?}");
        // Unknown extension is dropped.
        assert!(globs.iter().all(|g| !g.contains(".dat")), "{globs:?}");

        // DDL uses snake_case dir name.
        let toml = render_toml(&cfg);
        assert!(toml.contains("CREATE TABLE posts"), "{toml}");
        assert!(toml.contains("CREATE TABLE data"), "{toml}");
        assert!(toml.contains("CREATE TABLE notes"), "{toml}");
        // And it parses.
        crate::config::load_config_str(&toml).unwrap();
    }

    #[test]
    fn template_defaults_include_common_ignore_patterns() {
        let summary = DirectorySummary {
            root: PathBuf::from("/tmp"),
            patterns: vec![],
            truncated: false,
        };
        let cfg = template_from_summary(&summary);
        assert!(cfg.ignore.contains(&"node_modules/**".to_string()));
        assert!(cfg.ignore.contains(&".git/**".to_string()));
    }

    // ---- write_config ------------------------------------------------------

    #[test]
    fn write_config_refuses_to_overwrite_without_force() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".dirsql.toml");
        fs::write(&path, "old").unwrap();
        let err = write_config(&path, "new", false).unwrap_err();
        assert!(matches!(err, InferenceError::OutputExists(_)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn write_config_overwrites_with_force() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".dirsql.toml");
        fs::write(&path, "old").unwrap();
        write_config(&path, "new", true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn write_config_creates_missing_parent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested/dir/.dirsql.toml");
        write_config(&path, "x", false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "x");
    }

    // ---- helpers (sanitize / extension_of_glob) ---------------------------

    #[test]
    fn extension_of_glob_handles_leaf_only() {
        assert_eq!(extension_of_glob("*.json"), Some("json"));
        assert_eq!(extension_of_glob("data/*.csv"), Some("csv"));
        assert_eq!(extension_of_glob("README"), None);
        assert_eq!(extension_of_glob("dir/file"), None);
    }

    #[test]
    fn sanitize_ident_lowercases_and_strips_punct() {
        assert_eq!(sanitize_ident("Posts-Drafts"), "posts_drafts");
        assert_eq!(sanitize_ident("123nope"), "t_123nope");
        assert_eq!(sanitize_ident("$$$"), "items");
    }
}
