use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;

/// Result of matching a file path against a glob pattern with captures.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub table_name: String,
    pub captures: HashMap<String, String>,
}

/// A compiled glob pattern that may contain `{name}` capture placeholders.
struct PatternEntry {
    glob_set: GlobSet,
    table_name: String,
    /// Capture names in order of appearance in the pattern.
    capture_names: Vec<String>,
    /// Regex for extracting capture values from matched paths.
    /// None if pattern has no captures.
    capture_regex: Option<Regex>,
}

impl PatternEntry {
    /// Extract this pattern's `{name}` captures from `path`. Returns an empty
    /// map when the pattern declares no captures or the path does not match
    /// the capture regex.
    fn extract_captures(&self, path: &Path) -> HashMap<String, String> {
        let Some(regex) = &self.capture_regex else {
            return HashMap::new();
        };
        let path_str = path.to_string_lossy();
        regex
            .captures(&path_str)
            .map(|caps| {
                self.capture_names
                    .iter()
                    .filter_map(|name| {
                        caps.name(name)
                            .map(|m| (name.clone(), m.as_str().to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Maps file paths to table names based on glob patterns.
/// Every matching pattern fires: a file matching N patterns yields N
/// `MatchResult`s (one per table), so a file can belong to multiple tables.
/// An ignore list filters paths entirely. Supports `{name}` placeholders in
/// glob patterns that capture path segments.
pub struct TableMatcher {
    entries: Vec<PatternEntry>,
    ignore_set: GlobSet,
}

/// Parse `{name}` placeholders from a glob pattern.
/// Returns (glob_with_placeholders_replaced_by_star, capture_names, capture_regex).
pub fn parse_captures(pattern: &str) -> (String, Vec<String>, Option<Regex>) {
    let capture_re = Regex::new(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}").unwrap();
    let mut names = Vec::new();

    for cap in capture_re.captures_iter(pattern) {
        names.push(cap[1].to_string());
    }

    if names.is_empty() {
        return (pattern.to_string(), names, None);
    }

    let glob_pattern = capture_re.replace_all(pattern, "*").to_string();

    // Build a regex over the original pattern to extract capture values.
    let mut regex_parts = Vec::new();
    let mut last_end = 0;

    for mat in capture_re.find_iter(pattern) {
        let before = &pattern[last_end..mat.start()];
        regex_parts.push(glob_segment_to_regex(before));
        let name = &pattern[mat.start() + 1..mat.end() - 1];
        regex_parts.push(format!("(?P<{}>[^/]+)", name));
        last_end = mat.end();
    }
    let after = &pattern[last_end..];
    regex_parts.push(glob_segment_to_regex(after));

    let regex_str = format!("^{}$", regex_parts.join(""));
    let capture_regex = Regex::new(&regex_str).ok();

    (glob_pattern, names, capture_regex)
}

/// Convert a glob segment (no capture placeholders) to regex.
fn glob_segment_to_regex(segment: &str) -> String {
    let mut result = String::new();
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    // ** matches anything including /
                    if chars.peek() == Some(&'/') {
                        chars.next();
                        result.push_str("(?:.*/)?");
                    } else {
                        result.push_str(".*");
                    }
                } else {
                    result.push_str("[^/]*");
                }
            }
            '?' => result.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '@' | '%' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

impl TableMatcher {
    /// Build a new matcher from (glob_pattern, table_name) pairs and ignore patterns.
    /// Glob patterns may contain `{name}` placeholders that capture path segments.
    pub fn new(
        mappings: &[(&str, &str)],
        ignore_patterns: &[&str],
    ) -> Result<Self, globset::Error> {
        let mut entries = Vec::new();
        for (pattern, table_name) in mappings {
            let (glob_pattern, capture_names, capture_regex) = parse_captures(pattern);
            let mut builder = GlobSetBuilder::new();
            builder.add(Glob::new(&glob_pattern)?);
            entries.push(PatternEntry {
                glob_set: builder.build()?,
                table_name: table_name.to_string(),
                capture_names,
                capture_regex,
            });
        }

        let mut ignore_builder = GlobSetBuilder::new();
        for pattern in ignore_patterns {
            ignore_builder.add(Glob::new(pattern)?);
        }
        let ignore_set = ignore_builder.build()?;

        Ok(Self {
            entries,
            ignore_set,
        })
    }

    /// Returns one [`MatchResult`] per matching pattern, in declaration order.
    /// A file matching N patterns yields N results (fan-out); each result
    /// carries the captures from its own pattern's glob. Empty when nothing
    /// matches.
    pub fn match_all(&self, path: &Path) -> Vec<MatchResult> {
        self.entries
            .iter()
            .filter(|entry| entry.glob_set.is_match(path))
            .map(|entry| MatchResult {
                table_name: entry.table_name.clone(),
                captures: entry.extract_captures(path),
            })
            .collect()
    }

    /// Returns the captures that `table_name`'s own glob extracts from `path`.
    /// Used when re-parsing a file already known to belong to a table (e.g.
    /// the build/scan path), so captures stay per-glob rather than
    /// first-match. Empty when the table is unknown or declares no captures.
    pub fn captures_for(&self, path: &Path, table_name: &str) -> HashMap<String, String> {
        self.entries
            .iter()
            .find(|entry| entry.table_name == table_name)
            .map(|entry| entry.extract_captures(path))
            .unwrap_or_default()
    }

    /// Returns true if the path matches any ignore pattern.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore_set.is_match(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table names of every matching pattern, in declaration order.
    fn names(matcher: &TableMatcher, path: &str) -> Vec<String> {
        matcher
            .match_all(Path::new(path))
            .into_iter()
            .map(|m| m.table_name)
            .collect()
    }

    /// The single match for a one-table matcher (panics if zero matches).
    fn only(matcher: &TableMatcher, path: &str) -> MatchResult {
        let mut all = matcher.match_all(Path::new(path));
        assert_eq!(all.len(), 1, "expected exactly one match for {path}");
        all.remove(0)
    }

    #[test]
    fn match_all_returns_table_for_matching_glob() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        assert_eq!(names(&matcher, "report.csv"), vec!["data"]);
    }

    #[test]
    fn match_all_returns_empty_for_no_match() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        assert!(matcher.match_all(Path::new("readme.md")).is_empty());
    }

    #[test]
    fn all_matching_patterns_fire() {
        // Two patterns both matching one path fan out to both tables, in
        // declaration order.
        let matcher = TableMatcher::new(
            &[
                ("data/*/metadata.json", "ta"),
                ("data/*/metadata.json", "tb"),
            ],
            &[],
        )
        .unwrap();
        assert_eq!(
            names(&matcher, "data/2401.00001/metadata.json"),
            vec!["ta", "tb"],
        );
    }

    #[test]
    fn match_all_captures_are_per_pattern() {
        let matcher = TableMatcher::new(
            &[("data/{id}/metadata.json", "a"), ("**/metadata.json", "b")],
            &[],
        )
        .unwrap();
        let all = matcher.match_all(Path::new("data/x/metadata.json"));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].table_name, "a");
        assert_eq!(all[0].captures.get("id").map(String::as_str), Some("x"));
        assert_eq!(all[1].table_name, "b");
        assert!(
            all[1].captures.is_empty(),
            "the captureless glob yields no captures: {:?}",
            all[1].captures
        );
    }

    #[test]
    fn match_all_with_nested_path() {
        let matcher = TableMatcher::new(&[("**/*.jsonl", "events")], &[]).unwrap();
        assert_eq!(names(&matcher, "logs/2024/events.jsonl"), vec!["events"]);
    }

    #[test]
    fn is_ignored_returns_true_for_matching_pattern() {
        let matcher = TableMatcher::new(&[], &["*.tmp", ".git/**"]).unwrap();
        assert!(matcher.is_ignored(Path::new("scratch.tmp")));
        assert!(matcher.is_ignored(Path::new(".git/config")));
    }

    #[test]
    fn is_ignored_returns_false_for_non_matching_path() {
        let matcher = TableMatcher::new(&[], &["*.tmp"]).unwrap();
        assert!(!matcher.is_ignored(Path::new("data.csv")));
    }

    #[test]
    fn empty_matcher_matches_nothing() {
        let matcher = TableMatcher::new(&[], &[]).unwrap();
        assert!(matcher.match_all(Path::new("anything.txt")).is_empty());
        assert!(!matcher.is_ignored(Path::new("anything.txt")));
    }

    #[test]
    fn invalid_glob_returns_error() {
        let result = TableMatcher::new(&[("[invalid", "t")], &[]);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_ignore_pattern_returns_error() {
        let result = TableMatcher::new(&[], &["[invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn question_mark_matches_single_non_separator_char() {
        let matcher = TableMatcher::new(&[("file?.txt", "t")], &[]).unwrap();
        assert_eq!(names(&matcher, "file1.txt"), vec!["t"]);
        assert_eq!(names(&matcher, "fileA.txt"), vec!["t"]);
        assert!(matcher.match_all(Path::new("file.txt")).is_empty());
    }

    #[test]
    fn double_star_at_end_matches_any_depth() {
        let matcher = TableMatcher::new(&[("logs/**", "t")], &[]).unwrap();
        assert_eq!(names(&matcher, "logs/a.txt"), vec!["t"]);
        assert_eq!(names(&matcher, "logs/deep/nested/b.txt"), vec!["t"]);
    }

    #[test]
    fn capture_single_segment() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        let result = only(&matcher, "comments/abc123/index.jsonl");
        assert_eq!(result.table_name, "comments");
        assert_eq!(result.captures.get("thread_id").unwrap(), "abc123");
    }

    #[test]
    fn capture_multiple_segments() {
        let matcher = TableMatcher::new(&[("{org}/{repo}/data.json", "repos")], &[]).unwrap();
        let result = only(&matcher, "acme/widgets/data.json");
        assert_eq!(result.table_name, "repos");
        assert_eq!(result.captures.get("org").unwrap(), "acme");
        assert_eq!(result.captures.get("repo").unwrap(), "widgets");
    }

    #[test]
    fn no_captures_returns_empty_map() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        let result = only(&matcher, "report.csv");
        assert_eq!(result.table_name, "data");
        assert!(result.captures.is_empty());
    }

    #[test]
    fn capture_with_glob_star() {
        let matcher = TableMatcher::new(&[("logs/{date}/*.jsonl", "logs")], &[]).unwrap();
        let result = only(&matcher, "logs/2024-01-15/events.jsonl");
        assert_eq!(result.captures.get("date").unwrap(), "2024-01-15");
    }

    #[test]
    fn capture_no_match_returns_empty() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert!(matcher.match_all(Path::new("other/file.txt")).is_empty());
    }

    #[test]
    fn match_all_still_matches_pattern_with_captures() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert_eq!(
            names(&matcher, "comments/abc/index.jsonl"),
            vec!["comments"]
        );
    }

    #[test]
    fn capture_with_double_star() {
        let matcher = TableMatcher::new(&[("**/{category}/items.json", "items")], &[]).unwrap();
        let result = only(&matcher, "shop/electronics/items.json");
        assert_eq!(result.captures.get("category").unwrap(), "electronics");
    }

    #[test]
    fn capture_with_trailing_double_star() {
        let matcher = TableMatcher::new(&[("logs/{date}/**", "logs")], &[]).unwrap();
        let result = only(&matcher, "logs/2024-01-15/deep/events.jsonl");
        assert_eq!(result.table_name, "logs");
        assert_eq!(result.captures.get("date").unwrap(), "2024-01-15");
    }

    #[test]
    fn capture_with_question_mark() {
        let matcher = TableMatcher::new(&[("{name}?.txt", "files")], &[]).unwrap();
        let result = only(&matcher, "ab.txt");
        assert_eq!(result.table_name, "files");
        assert!(result.captures.contains_key("name"));
    }

    #[test]
    fn captures_for_returns_the_named_tables_captures() {
        let matcher = TableMatcher::new(
            &[("data/{id}/metadata.json", "a"), ("**/metadata.json", "b")],
            &[],
        )
        .unwrap();
        let path = Path::new("data/x/metadata.json");
        let a_caps = matcher.captures_for(path, "a");
        assert_eq!(a_caps.get("id").map(String::as_str), Some("x"));
        // `b`'s glob has no captures, so its per-table lookup is empty even
        // though the same path matches it.
        assert!(matcher.captures_for(path, "b").is_empty());
    }

    #[test]
    fn captures_for_unknown_table_is_empty() {
        let matcher = TableMatcher::new(&[("data/{id}/metadata.json", "a")], &[]).unwrap();
        assert!(
            matcher
                .captures_for(Path::new("data/x/metadata.json"), "nope")
                .is_empty()
        );
    }
}
