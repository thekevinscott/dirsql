use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::Path;

/// Maps file paths to table names based on glob patterns.
/// First matching pattern wins. An ignore list filters paths entirely.
pub struct TableMatcher {
    table_globs: Vec<(GlobSet, String)>,
    ignore_set: GlobSet,
}

impl TableMatcher {
    /// Build a new matcher from (glob_pattern, table_name) pairs and ignore patterns.
    pub fn new(
        mappings: &[(&str, &str)],
        ignore_patterns: &[&str],
    ) -> Result<Self, globset::Error> {
        let mut table_globs = Vec::new();
        for (pattern, table_name) in mappings {
            let mut builder = GlobSetBuilder::new();
            builder.add(Glob::new(pattern)?);
            table_globs.push((builder.build()?, table_name.to_string()));
        }

        let mut ignore_builder = GlobSetBuilder::new();
        for pattern in ignore_patterns {
            ignore_builder.add(Glob::new(pattern)?);
        }
        let ignore_set = ignore_builder.build()?;

        Ok(Self {
            table_globs,
            ignore_set,
        })
    }

    /// Returns the table name for a file path, or None if no pattern matches.
    pub fn match_file(&self, path: &Path) -> Option<&str> {
        for (glob_set, table_name) in &self.table_globs {
            if glob_set.is_match(path) {
                return Some(table_name.as_str());
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
}
