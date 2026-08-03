use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Result of matching a file path against a glob pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub table_name: String,
}

/// A compiled glob pattern. `{name}` placeholders are rewritten to `*` before
/// compilation, so they are pure match wildcards.
struct PatternEntry {
    glob_set: GlobSet,
    table_name: String,
}

/// Maps file paths to table names based on glob patterns.
/// Every matching pattern fires: a file matching N patterns yields N
/// `MatchResult`s (one per table), so a file can belong to multiple tables.
/// An ignore list filters paths entirely. `{name}` placeholders in glob
/// patterns are accepted and behave like `*`.
pub struct TableMatcher {
    entries: Vec<PatternEntry>,
    ignore_set: GlobSet,
    ignore_dir_set: GlobSet,
}

/// Byte spans of the `{name}` placeholders in `pattern`, in order: `(start,
/// end, name)` with `end` past the closing brace.
///
/// The grammar is exactly `{` `[a-zA-Z_][a-zA-Z0-9_]*` `}`. Anything else --
/// `{}`, `{1a}`, `{a-b}`, an unclosed `{` -- is not a placeholder and is left
/// alone, matching the leftmost-first, non-overlapping scan the equivalent
/// regex performed.
fn placeholder_spans(pattern: &str) -> Vec<(usize, usize, String)> {
    let bytes = pattern.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut j = i + 1;
            if j < bytes.len() && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                j += 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'}' {
                    // Every byte in `i..=j` is ASCII, so these are char boundaries.
                    spans.push((i, j + 1, pattern[i + 1..j].to_string()));
                    i = j + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    spans
}

/// Names of the `{name}` placeholders in `pattern`, in order of appearance.
pub fn placeholder_names(pattern: &str) -> Vec<String> {
    placeholder_spans(pattern)
        .into_iter()
        .map(|(_, _, name)| name)
        .collect()
}

/// Rewrite `{name}` placeholders in a glob to `*`, so they match a single path
/// segment without producing any captured value.
fn glob_with_placeholders_as_star(pattern: &str) -> String {
    let spans = placeholder_spans(pattern);
    if spans.is_empty() {
        return pattern.to_string();
    }
    let mut out = String::with_capacity(pattern.len());
    let mut last = 0;
    for (start, end, _) in spans {
        out.push_str(&pattern[last..start]);
        out.push('*');
        last = end;
    }
    out.push_str(&pattern[last..]);
    out
}

impl TableMatcher {
    /// Build a new matcher from (glob_pattern, table_name) pairs and ignore patterns.
    /// Glob patterns may contain `{name}` placeholders, which match like `*`.
    pub fn new(
        mappings: &[(&str, &str)],
        ignore_patterns: &[&str],
    ) -> Result<Self, globset::Error> {
        let mut entries = Vec::new();
        for (pattern, table_name) in mappings {
            let glob_pattern = glob_with_placeholders_as_star(pattern);
            let mut builder = GlobSetBuilder::new();
            builder.add(Glob::new(&glob_pattern)?);
            entries.push(PatternEntry {
                glob_set: builder.build()?,
                table_name: table_name.to_string(),
            });
        }

        let mut ignore_builder = GlobSetBuilder::new();
        let mut ignore_dir_builder = GlobSetBuilder::new();
        for pattern in ignore_patterns {
            ignore_builder.add(Glob::new(pattern)?);
            if let Some(subtree) = pattern.strip_suffix("/**") {
                ignore_dir_builder.add(Glob::new(subtree)?);
            }
        }
        let ignore_set = ignore_builder.build()?;
        let ignore_dir_set = ignore_dir_builder.build()?;

        Ok(Self {
            entries,
            ignore_set,
            ignore_dir_set,
        })
    }

    /// Returns one [`MatchResult`] per matching pattern, in declaration order.
    /// A file matching N patterns yields N results (fan-out). Empty when
    /// nothing matches.
    pub fn match_all(&self, path: &Path) -> Vec<MatchResult> {
        self.entries
            .iter()
            .filter(|entry| entry.glob_set.is_match(path))
            .map(|entry| MatchResult {
                table_name: entry.table_name.clone(),
            })
            .collect()
    }

    /// Returns true if the path matches any ignore pattern.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore_set.is_match(path)
    }

    /// True when an ignore pattern covers the whole subtree beneath `dir`
    /// (the `<dir>/**` form), so a walk may skip the directory without
    /// reading it.
    pub(crate) fn is_ignored_dir(&self, dir: &Path) -> bool {
        self.ignore_dir_set.is_match(dir)
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
    fn placeholder_glob_matches_like_a_star() {
        // A `{name}` placeholder and a `*` compile to the same matcher: both
        // globs match exactly the same file.
        let placeholder = TableMatcher::new(&[("data/{id}/metadata.json", "a")], &[]).unwrap();
        let star = TableMatcher::new(&[("data/*/metadata.json", "a")], &[]).unwrap();
        let path = Path::new("data/x/metadata.json");
        assert_eq!(names(&placeholder, "data/x/metadata.json"), vec!["a"]);
        assert_eq!(
            placeholder.match_all(path).is_empty(),
            star.match_all(path).is_empty(),
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
    fn a_leading_double_star_ignore_matches_at_any_depth_including_the_top() {
        let matcher = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(matcher.is_ignored(Path::new("node_modules/pkg/index.js")));
        assert!(matcher.is_ignored(Path::new("apps/site/node_modules/pkg/index.js")));
    }

    #[test]
    fn is_ignored_dir_matches_the_directory_a_subtree_pattern_covers() {
        let matcher = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(matcher.is_ignored_dir(Path::new("node_modules")));
        assert!(matcher.is_ignored_dir(Path::new("apps/site/node_modules")));
    }

    #[test]
    fn is_ignored_dir_rejects_a_directory_no_subtree_pattern_covers() {
        let matcher = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(!matcher.is_ignored_dir(Path::new("docs")));
        assert!(!matcher.is_ignored_dir(Path::new("node_modules/pkg")));
    }

    #[test]
    fn a_non_subtree_ignore_pattern_never_marks_a_directory() {
        let matcher = TableMatcher::new(&[], &["*.tmp"]).unwrap();
        assert!(!matcher.is_ignored_dir(Path::new("scratch.tmp")));
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
    fn placeholder_matches_the_filled_segment() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert_eq!(
            names(&matcher, "comments/abc123/index.jsonl"),
            vec!["comments"]
        );
    }

    #[test]
    fn multiple_placeholders_match() {
        let matcher = TableMatcher::new(&[("{org}/{repo}/data.json", "repos")], &[]).unwrap();
        assert_eq!(names(&matcher, "acme/widgets/data.json"), vec!["repos"]);
    }

    #[test]
    fn placeholder_no_match_returns_empty() {
        let matcher =
            TableMatcher::new(&[("comments/{thread_id}/index.jsonl", "comments")], &[]).unwrap();
        assert!(matcher.match_all(Path::new("other/file.txt")).is_empty());
    }

    #[test]
    fn placeholder_with_double_star_matches() {
        let matcher = TableMatcher::new(&[("**/{category}/items.json", "items")], &[]).unwrap();
        assert_eq!(
            names(&matcher, "shop/electronics/items.json"),
            vec!["items"]
        );
    }

    #[test]
    fn placeholder_with_question_mark_matches() {
        let matcher = TableMatcher::new(&[("{name}?.txt", "files")], &[]).unwrap();
        assert_eq!(names(&matcher, "ab.txt"), vec!["files"]);
    }

    #[test]
    fn placeholder_names_lists_names_in_order() {
        assert_eq!(
            placeholder_names("{org}/{repo}/data.json"),
            vec!["org".to_string(), "repo".to_string()]
        );
    }

    #[test]
    fn placeholder_names_empty_when_none() {
        assert!(placeholder_names("data/*/metadata.json").is_empty());
    }
}
