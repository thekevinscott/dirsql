//! Integration tests for `scan_directory`.
//!
//! These build real temp directory trees with real files and assert what the
//! scanner returns end-to-end through its public API. They were moved out of
//! `scanner.rs`'s inline `#[cfg(test)]` module so that module stays pure (the
//! `testing-conventions` `unit lint` isolation rule forbids effectful std --
//! `std::fs`, temp dirs -- in unit tests). The pure reserved-dir predicate
//! test remains inline next to the function it covers.

use std::fs;

use dirsql::matcher::TableMatcher;
use dirsql::scanner::scan_directory;
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

#[test]
fn scan_excludes_top_level_dirsql_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("real.csv"), "a,b\n1,2").unwrap();

    // Files inside the reserved `.dirsql/` directory (e.g. the cache db)
    // must never be picked up by the scanner.
    fs::create_dir(dir.path().join(".dirsql")).unwrap();
    fs::write(dir.path().join(".dirsql").join("cache.csv"), "a,b\n1,2").unwrap();

    let matcher = TableMatcher::new(&[("**/*.csv", "t")], &[]).unwrap();
    let results = scan_directory(dir.path(), &matcher);

    assert_eq!(results.len(), 1);
    assert!(results[0].0.ends_with("real.csv"));
}
