use crate::matcher::TableMatcher;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Walk a directory tree and return all file paths paired with their matching table name.
/// Ignored paths and directories are skipped. Only files (not directories) are returned.
pub fn scan_directory(root: &Path, matcher: &TableMatcher) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();

        if matcher.is_ignored(path) {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        if let Some(table_name) = matcher.match_file(path) {
            results.push((path.to_path_buf(), table_name.to_string()));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn scan_finds_matching_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("data.csv"), "a,b\n1,2").unwrap();
        fs::write(dir.path().join("readme.md"), "# hi").unwrap();

        let matcher = TableMatcher::new(&[("**/*.csv", "csv_table")], &[]).unwrap();
        let results = scan_directory(dir.path(), &matcher);

        assert_eq!(results.len(), 1);
        assert!(results[0].0.ends_with("data.csv"));
        assert_eq!(results[0].1, "csv_table");
    }

    #[test]
    fn scan_skips_ignored_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("data.csv"), "a,b").unwrap();
        fs::write(dir.path().join("data.tmp"), "junk").unwrap();

        let matcher =
            TableMatcher::new(&[("**/*.csv", "t"), ("**/*.tmp", "t2")], &["**/*.tmp"]).unwrap();
        let results = scan_directory(dir.path(), &matcher);

        assert_eq!(results.len(), 1);
        assert!(results[0].0.ends_with("data.csv"));
    }

    #[test]
    fn scan_recurses_into_subdirectories() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("nested").join("deep");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("events.jsonl"), "{}").unwrap();

        let matcher = TableMatcher::new(&[("**/*.jsonl", "events")], &[]).unwrap();
        let results = scan_directory(dir.path(), &matcher);

        assert_eq!(results.len(), 1);
        assert!(results[0].0.ends_with("events.jsonl"));
        assert_eq!(results[0].1, "events");
    }

    #[test]
    fn scan_returns_empty_for_no_matches() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("readme.md"), "# hi").unwrap();

        let matcher = TableMatcher::new(&[("**/*.csv", "t")], &[]).unwrap();
        let results = scan_directory(dir.path(), &matcher);

        assert!(results.is_empty());
    }

    #[test]
    fn scan_skips_directories() {
        let dir = TempDir::new().unwrap();
        // Create a directory that matches the glob -- it should not appear in results
        fs::create_dir(dir.path().join("data.csv")).unwrap();

        let matcher = TableMatcher::new(&[("**/*.csv", "t")], &[]).unwrap();
        let results = scan_directory(dir.path(), &matcher);

        assert!(results.is_empty());
    }
}
