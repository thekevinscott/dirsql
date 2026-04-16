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

/// Maps file paths to table names based on glob patterns.
/// First matching pattern wins. An ignore list filters paths entirely.
/// Supports `{name}` placeholders in glob patterns that capture path segments.
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

    // Replace {name} with * for glob matching
    let glob_pattern = capture_re.replace_all(pattern, "*").to_string();

    // Build a regex that captures the values from matched paths.
    // Escape everything except our capture groups, and replace {name} with a named group.
    let mut regex_parts = Vec::new();
    let mut last_end = 0;

    for mat in capture_re.find_iter(pattern) {
        let before = &pattern[last_end..mat.start()];
        // Convert glob syntax in the "before" segment to regex
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

    /// Returns the table name for a file path, or None if no pattern matches.
    /// For backward compatibility -- does not return captures.
    pub fn match_file(&self, path: &Path) -> Option<&str> {
        for entry in &self.entries {
            if entry.glob_set.is_match(path) {
                return Some(entry.table_name.as_str());
            }
        }
        None
    }

    /// Returns a MatchResult with table name and any captured path segments,
    /// or None if no pattern matches.
    pub fn match_file_with_captures(&self, path: &Path) -> Option<MatchResult> {
        for entry in &self.entries {
            if entry.glob_set.is_match(path) {
                let captures = if let Some(ref regex) = entry.capture_regex {
                    let path_str = path.to_string_lossy();
                    if let Some(caps) = regex.captures(&path_str) {
                        entry
                            .capture_names
                            .iter()
                            .filter_map(|name| {
                                caps.name(name)
                                    .map(|m| (name.clone(), m.as_str().to_string()))
                            })
                            .collect()
                    } else {
                        HashMap::new()
                    }
                } else {
                    HashMap::new()
                };
                return Some(MatchResult {
                    table_name: entry.table_name.clone(),
                    captures,
                });
            }
        }
        None
    }

    /// Returns true if the path matches any ignore pattern.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore_set.is_match(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_file_returns_table_for_matching_glob() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        assert_eq!(matcher.match_file(Path::new("report.csv")), Some("data"));
    }

    #[test]
    fn match_file_returns_none_for_no_match() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        assert_eq!(matcher.match_file(Path::new("readme.md")), None);
    }

    #[test]
    fn first_matching_pattern_wins() {
        let matcher = TableMatcher::new(
            &[("*.json", "json_table"), ("data/*.json", "data_table")],
            &[],
        )
        .unwrap();
        // "data/foo.json" matches *.json first
        assert_eq!(
            matcher.match_file(Path::new("data/foo.json")),
            Some("json_table")
        );
    }

    #[test]
    fn match_file_with_nested_path() {
        let matcher = TableMatcher::new(&[("**/*.jsonl", "events")], &[]).unwrap();
        assert_eq!(
            matcher.match_file(Path::new("logs/2024/events.jsonl")),
            Some("events")
        );
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
        assert_eq!(matcher.match_file(Path::new("anything.txt")), None);
        assert!(!matcher.is_ignored(Path::new("anything.txt")));
    }

    #[test]
    fn invalid_glob_returns_error() {
        let result = TableMatcher::new(&[("[invalid", "t")], &[]);
        assert!(result.is_err());
    }

    // --- Path capture tests ---

    #[test]
    fn capture_single_segment() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        let result = matcher
            .match_file_with_captures(Path::new("comments/abc123/index.jsonl"))
            .unwrap();
        assert_eq!(result.table_name, "comments");
        assert_eq!(result.captures.get("thread_id").unwrap(), "abc123");
    }

    #[test]
    fn capture_multiple_segments() {
        let matcher = TableMatcher::new(&[("{org}/{repo}/data.json", "repos")], &[]).unwrap();
        let result = matcher
            .match_file_with_captures(Path::new("acme/widgets/data.json"))
            .unwrap();
        assert_eq!(result.table_name, "repos");
        assert_eq!(result.captures.get("org").unwrap(), "acme");
        assert_eq!(result.captures.get("repo").unwrap(), "widgets");
    }

    #[test]
    fn no_captures_returns_empty_map() {
        let matcher = TableMatcher::new(&[("*.csv", "data")], &[]).unwrap();
        let result = matcher
            .match_file_with_captures(Path::new("report.csv"))
            .unwrap();
        assert_eq!(result.table_name, "data");
        assert!(result.captures.is_empty());
    }

    #[test]
    fn capture_with_glob_star() {
        let matcher = TableMatcher::new(&[("logs/{date}/*.jsonl", "logs")], &[]).unwrap();
        let result = matcher
            .match_file_with_captures(Path::new("logs/2024-01-15/events.jsonl"))
            .unwrap();
        assert_eq!(result.captures.get("date").unwrap(), "2024-01-15");
    }

    #[test]
    fn capture_no_match_returns_none() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert!(
            matcher
                .match_file_with_captures(Path::new("other/file.txt"))
                .is_none()
        );
    }

    #[test]
    fn match_file_still_works_with_captures_in_pattern() {
        // The old match_file API should still work when patterns have captures
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert_eq!(
            matcher.match_file(Path::new("comments/abc/index.jsonl")),
            Some("comments")
        );
    }

    #[test]
    fn capture_with_double_star() {
        let matcher = TableMatcher::new(&[("**/{category}/items.json", "items")], &[]).unwrap();
        let result = matcher
            .match_file_with_captures(Path::new("shop/electronics/items.json"))
            .unwrap();
        assert_eq!(result.captures.get("category").unwrap(), "electronics");
    }
}
