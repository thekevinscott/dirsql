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
        assert!(matcher.match_all(Path::new("file/.txt")).is_empty());
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

    #[test]
    fn placeholder_names_rejects_everything_outside_the_grammar() {
        // The grammar is exactly `{` `[a-zA-Z_][a-zA-Z0-9_]*` `}`. Each of these
        // fails it at a different point -- empty name, a leading digit, a
        // disallowed interior byte, and a brace that runs off the end of the
        // pattern -- and all are left alone rather than read as a placeholder.
        for pattern in ["{}", "{1a}", "{a-b}", "{a b}", "{", "a{", "{a", "{_"] {
            assert!(
                placeholder_names(pattern).is_empty(),
                "`{pattern}` is outside the grammar, so it is not a placeholder"
            );
            assert_eq!(
                glob_with_placeholders_as_star(pattern),
                pattern,
                "`{pattern}` is not a placeholder, so the glob is unrewritten"
            );
        }
    }

    #[test]
    fn placeholder_names_accepts_the_whole_grammar() {
        // A name starts with a letter or `_` and continues with those plus
        // digits.
        assert_eq!(placeholder_names("{a}"), vec!["a".to_string()]);
        assert_eq!(placeholder_names("{_}"), vec!["_".to_string()]);
        assert_eq!(placeholder_names("{_a1}"), vec!["_a1".to_string()]);
        assert_eq!(placeholder_names("{A_1z}"), vec!["A_1z".to_string()]);
    }

    #[test]
    fn the_scan_resumes_at_the_byte_after_a_closing_brace() {
        // Leftmost-first and non-overlapping: back-to-back placeholders both
        // register, and every byte outside a span survives the rewrite.
        assert_eq!(
            placeholder_names("{a}{b}"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(glob_with_placeholders_as_star("x{a}y{b}z"), "x*y*z");
    }

    #[test]
    fn an_unclosed_brace_leaves_a_later_placeholder_intact() {
        // The `{` at 0 fails the grammar, so the scan advances one byte at a
        // time and still finds the real placeholder behind it.
        assert_eq!(placeholder_names("{a{b}"), vec!["b".to_string()]);
        assert_eq!(glob_with_placeholders_as_star("{a{b}"), "{a*");
    }

    /// The nested fixture the #1223 table was measured against: two files at
    /// depth 0, two under `folder/`, two under `folder/sub/`, one under a
    /// sibling directory.
    const FIXTURE: [&str; 7] = [
        "root.md",
        "root.txt",
        "folder/a.md",
        "folder/a.txt",
        "folder/sub/b.md",
        "folder/sub/b.txt",
        "sibling/c.md",
    ];

    fn matched(glob: &str) -> Vec<&'static str> {
        let matcher = TableMatcher::new(&[(glob, "t")], &[]).unwrap();
        FIXTURE
            .into_iter()
            .filter(|p| !matcher.match_all(Path::new(p)).is_empty())
            .collect()
    }

    fn hidden(ignore: &str) -> Vec<&'static str> {
        let matcher = TableMatcher::new(&[], &[ignore]).unwrap();
        FIXTURE
            .into_iter()
            .filter(|p| matcher.is_ignored(Path::new(p)))
            .collect()
    }

    #[test]
    fn a_lone_star_glob_matches_depth_zero_only() {
        assert_eq!(matched("*"), ["root.md", "root.txt"]);
    }

    #[test]
    fn a_bare_directory_name_glob_matches_nothing() {
        assert!(matched("folder").is_empty());
    }

    #[test]
    fn a_directory_slash_star_glob_stops_at_the_next_separator() {
        assert_eq!(matched("folder/*"), ["folder/a.md", "folder/a.txt"]);
    }

    #[test]
    fn a_directory_slash_double_star_glob_matches_any_depth_below() {
        assert_eq!(
            matched("folder/**"),
            [
                "folder/a.md",
                "folder/a.txt",
                "folder/sub/b.md",
                "folder/sub/b.txt"
            ]
        );
    }

    #[test]
    fn a_leading_double_star_glob_matches_the_suffix_at_any_depth() {
        assert_eq!(
            matched("**/*.md"),
            ["root.md", "folder/a.md", "folder/sub/b.md", "sibling/c.md"]
        );
    }

    #[test]
    fn a_double_star_slash_star_glob_matches_every_file() {
        assert_eq!(matched("**/*"), FIXTURE);
    }

    #[test]
    fn a_directory_slash_star_ignore_hides_only_that_directory_s_files() {
        assert_eq!(hidden("folder/*"), ["folder/a.md", "folder/a.txt"]);
    }

    #[test]
    fn a_lone_star_ignore_hides_depth_zero_only() {
        assert_eq!(hidden("*"), ["root.md", "root.txt"]);
    }

    #[test]
    fn the_default_ignores_still_hide_at_every_depth() {
        let matcher = TableMatcher::new(&[], &["**/node_modules/**", "**/.git/**"]).unwrap();
        for path in [
            "node_modules/pkg/index.js",
            "apps/site/node_modules/pkg/dist/index.js",
            ".git/config",
            "vendor/lib/.git/HEAD",
        ] {
            assert!(
                matcher.is_ignored(Path::new(path)),
                "{path} must stay hidden"
            );
        }
    }

    #[test]
    fn a_subtree_ignore_whose_prefix_has_a_star_marks_one_level_of_directories() {
        let matcher = TableMatcher::new(&[], &["folder/*/**"]).unwrap();
        assert!(matcher.is_ignored_dir(Path::new("folder/sub")));
        assert!(!matcher.is_ignored_dir(Path::new("folder/sub/deeper")));
    }

    #[test]
    fn a_placeholder_matches_exactly_one_path_segment() {
        let matcher = TableMatcher::new(&[("data/{id}/metadata.json", "a")], &[]).unwrap();
        assert_eq!(names(&matcher, "data/x/metadata.json"), vec!["a"]);
        assert!(
            matcher
                .match_all(Path::new("data/x/y/metadata.json"))
                .is_empty()
        );
    }
}
