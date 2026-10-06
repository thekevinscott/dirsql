use crate::posix_class::expand_posix_classes;
use crate::scanner::{PathGlob, compile_glob, compile_globs};
use globset::GlobBuilder;
use regex::{Regex, RegexBuilder};
use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum GlobError {
    #[error(transparent)]
    Glob(#[from] globset::Error),
    #[error(transparent)]
    Regex(#[from] regex::Error),
}

/// A glob whose `?` and bracket expressions match one character, as bash
/// does in a UTF-8 locale. globset parses the pattern, but its regex is
/// byte-oriented, so it is recompiled in Unicode mode over the `/`-separated
/// path text.
#[derive(Debug)]
pub(crate) struct Pattern(Regex);

impl Pattern {
    /// `*` and `?` stop at `/`; `**` crosses it.
    pub(crate) fn new(glob: &str) -> Result<Self, GlobError> {
        let glob = GlobBuilder::new(&expand_posix_classes(glob))
            .literal_separator(true)
            .build()?;
        let regex = RegexBuilder::new(&unicode_regex(glob.regex()))
            .dot_matches_new_line(true)
            .build()?;
        Ok(Self(regex))
    }

    pub(crate) fn is_match(&self, path: &Path) -> bool {
        self.0.is_match(&crate::scanner::to_slash(path))
    }
}

/// globset spells each non-ASCII character as the `\xNN` escapes of its UTF-8
/// bytes, which in Unicode mode would name code points instead; decode each
/// run back into the characters it encodes.
fn unicode_regex(byte_regex: &str) -> String {
    let body = byte_regex
        .strip_prefix("(?-u)")
        .unwrap_or(byte_regex)
        .as_bytes();
    let mut out = Vec::with_capacity(body.len());
    let mut rest = body;
    loop {
        rest = match rest {
            [b'\\', b'x', hi, lo, tail @ ..] => {
                out.push(hex_byte([*hi, *lo]));
                tail
            }
            [b'\\', escaped, tail @ ..] => {
                out.extend([b'\\', *escaped]);
                tail
            }
            [b, tail @ ..] => {
                out.push(*b);
                tail
            }
            [] => break,
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(digits: [u8; 2]) -> u8 {
    std::str::from_utf8(&digits)
        .ok()
        .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        .unwrap_or_default()
}

/// Result of matching a file path against a glob pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    pub table_name: String,
}

const NODE_MODULES: &str = "node_modules";

struct PatternEntry {
    pattern: PathGlob,
    table_name: String,
    names_node_modules: bool,
}

/// Maps file paths to table names based on glob patterns.
/// Every matching pattern fires: a file matching N patterns yields N
/// `MatchResult`s (one per table), so a file can belong to multiple tables.
/// An ignore list filters paths entirely.
pub struct TableMatcher {
    entries: Vec<PatternEntry>,
    ignore_set: Vec<Pattern>,
    ignore_dir_set: Vec<Pattern>,
    gitignore: bool,
    walk: PathGlob,
}

fn has_node_modules_component(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == NODE_MODULES)
}

impl TableMatcher {
    /// Build a new matcher from (glob_pattern, table_name) pairs and ignore patterns.
    pub fn new(mappings: &[(&str, &str)], ignore_patterns: &[&str]) -> Result<Self, GlobError> {
        let mut entries = Vec::new();
        for (pattern, table_name) in mappings {
            entries.push(PatternEntry {
                pattern: compile_glob(pattern)?,
                table_name: table_name.to_string(),
                names_node_modules: pattern.split('/').any(|c| c == NODE_MODULES),
            });
        }
        let globs: Vec<&str> = mappings.iter().map(|(pattern, _)| *pattern).collect();
        let walk = compile_globs(&globs)?;

        let mut ignore_set = Vec::new();
        let mut ignore_dir_set = Vec::new();
        for pattern in ignore_patterns {
            ignore_set.push(Pattern::new(pattern)?);
            if let Some(subtree) = pattern.strip_suffix("/**") {
                ignore_dir_set.push(Pattern::new(subtree)?);
            }
        }

        Ok(Self {
            entries,
            ignore_set,
            ignore_dir_set,
            gitignore: false,
            walk,
        })
    }

    /// Whether a scan under this matcher honors `.gitignore` files.
    pub fn with_gitignore(mut self, gitignore: bool) -> Self {
        self.gitignore = gitignore;
        self
    }

    pub(crate) fn respects_gitignore(&self) -> bool {
        self.gitignore
    }

    /// Returns one [`MatchResult`] per matching pattern, in declaration order.
    /// A file matching N patterns yields N results (fan-out). Empty when
    /// nothing matches.
    pub fn match_all(&self, path: &Path) -> Vec<MatchResult> {
        self.entries
            .iter()
            .filter(|entry| {
                entry.pattern.is_match_unhidden(path)
                    && (entry.names_node_modules || !has_node_modules_component(path))
            })
            .map(|entry| MatchResult {
                table_name: entry.table_name.clone(),
            })
            .collect()
    }

    /// The one glob a walk for every table at once follows.
    pub(crate) fn walk_glob(&self) -> &PathGlob {
        &self.walk
    }

    /// Whether the walk may skip a directory called `name` outright: it is
    /// `node_modules` and no table's glob names it.
    pub(crate) fn prunes_directory(&self, name: &OsStr) -> bool {
        name == NODE_MODULES
            && !self.entries.is_empty()
            && !self.entries.iter().any(|e| e.names_node_modules)
    }

    /// Returns true if the path matches any ignore pattern.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.ignore_set.iter().any(|p| p.is_match(path))
    }

    /// True when an ignore pattern covers the whole subtree beneath `dir`
    /// (the `<dir>/**` form), so a walk may skip the directory without
    /// reading it.
    pub(crate) fn is_ignored_dir(&self, dir: &Path) -> bool {
        self.ignore_dir_set.iter().any(|p| p.is_match(dir))
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
    fn a_matcher_honors_gitignore_only_once_asked_to() {
        let matcher = TableMatcher::new(&[], &[]).unwrap();
        assert!(!matcher.respects_gitignore());
        assert!(matcher.with_gitignore(true).respects_gitignore());
    }

    #[test]
    fn node_modules_is_skipped_unless_a_table_names_it() {
        let matcher =
            TableMatcher::new(&[("**/*.js", "all"), ("node_modules/*.js", "nm")], &[]).unwrap();
        assert_eq!(names(&matcher, "pkg/a.js"), vec!["all"]);
        assert_eq!(
            names(&matcher, "pkg/node_modules/a.js"),
            Vec::<String>::new()
        );
        assert_eq!(names(&matcher, "node_modules/a.js"), vec!["nm"]);
    }

    #[test]
    fn the_walk_prunes_node_modules_only_while_no_table_names_it() {
        let name = OsStr::new("node_modules");
        let plain = TableMatcher::new(&[("**/*.js", "t")], &[]).unwrap();
        let naming = TableMatcher::new(&[("node_modules/*.js", "t")], &[]).unwrap();
        let none = TableMatcher::new(&[], &[]).unwrap();
        assert!(plain.prunes_directory(name));
        assert!(!plain.prunes_directory(OsStr::new("src")));
        assert!(!naming.prunes_directory(name));
        assert!(!none.prunes_directory(name));
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

    /// Two files at depth 0, two under `folder/`, two under `folder/sub/`, one
    /// under a sibling directory.
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
    fn a_question_mark_matches_one_character_not_one_byte() {
        let matcher = TableMatcher::new(&[("caf?.md", "t"), ("na??ve.txt", "u")], &[]).unwrap();
        assert_eq!(names(&matcher, "caf\u{e9}.md"), vec!["t"]);
        assert!(matcher.match_all(Path::new("na\u{ef}ve.txt")).is_empty());
    }

    #[test]
    fn unicode_regex_decodes_byte_escapes_into_characters() {
        assert_eq!(
            unicode_regex(r"(?-u)^caf\xc3\xa9[\xc3\xa0-\xc3\xaa]$"),
            "^café[à-ê]$"
        );
    }

    #[test]
    fn unicode_regex_keeps_an_escaped_backslash_before_an_x() {
        assert_eq!(unicode_regex(r"(?-u)^\\x41\.md$"), r"^\\x41\.md$");
    }

    #[test]
    fn a_pattern_too_large_for_a_unicode_regex_is_an_error() {
        let err = Pattern::new(&"?".repeat(200_000)).unwrap_err();
        assert!(matches!(err, GlobError::Regex(_)), "{err:?}");
    }

    #[test]
    fn a_bracket_expression_matches_a_non_ascii_character() {
        let matcher = TableMatcher::new(&[("na[\u{ef}]ve.txt", "t")], &[]).unwrap();
        assert_eq!(names(&matcher, "na\u{ef}ve.txt"), vec!["t"]);
    }

    #[test]
    fn a_braced_name_matches_only_itself() {
        let matcher = TableMatcher::new(&[("data/{id}/metadata.json", "a")], &[]).unwrap();
        assert_eq!(names(&matcher, "data/{id}/metadata.json"), vec!["a"]);
        assert!(
            matcher
                .match_all(Path::new("data/x/metadata.json"))
                .is_empty()
        );
    }

    #[test]
    fn a_posix_class_in_a_table_glob_matches_its_characters() {
        let matcher = TableMatcher::new(&[("notes/[[:digit:]].md", "n")], &[]).unwrap();
        assert_eq!(names(&matcher, "notes/7.md"), vec!["n"]);
        assert!(matcher.match_all(Path::new("notes/x.md")).is_empty());
    }
}
