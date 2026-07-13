//! Integration tests for `scan_directory`: real temp trees, real files,
//! end-to-end through the public API (the unit-lint isolation rule keeps
//! effectful std out of the inline unit module).

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
    fs::create_dir(dir.path().join("data.csv")).unwrap();

    let matcher = TableMatcher::new(&[("**/*.csv", "t")], &[]).unwrap();
    let results = scan_directory(dir.path(), &matcher);

    assert!(results.is_empty());
}

// A file matching two tables' globs must produce one (path, table) pair
// per matching table (fan-out), in declaration order — not a single
// first-match pair.
#[test]
fn scan_fans_out_file_matching_two_tables() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("data").join("2401.00001");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("metadata.json"), "{}").unwrap();

    let matcher = TableMatcher::new(
        &[
            ("data/*/metadata.json", "ta"),
            ("data/*/metadata.json", "tb"),
        ],
        &[],
    )
    .unwrap();
    let results = scan_directory(dir.path(), &matcher);

    assert_eq!(results.len(), 2, "one pair per matching table: {results:?}");
    let tables: Vec<&str> = results.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(tables, vec!["ta", "tb"], "pairs in declaration order");
    assert!(
        results
            .iter()
            .all(|(p, _)| p.ends_with("data/2401.00001/metadata.json")),
        "both pairs point at the same file: {results:?}"
    );
}

// Distinct-but-overlapping globs (`data/*/…` and `data/**/…`) both matching
// one file must also fan out to both tables.
#[test]
fn scan_fans_out_overlapping_distinct_globs() {
    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("data").join("2401.00001");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("metadata.json"), "{}").unwrap();

    let matcher = TableMatcher::new(
        &[
            ("data/*/metadata.json", "ta"),
            ("data/**/metadata.json", "tb"),
        ],
        &[],
    )
    .unwrap();
    let results = scan_directory(dir.path(), &matcher);

    let tables: Vec<&str> = results.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(tables, vec!["ta", "tb"], "both tables matched: {results:?}");
}

#[test]
fn scan_excludes_top_level_dirsql_directory() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("real.csv"), "a,b\n1,2").unwrap();

    fs::create_dir(dir.path().join(".dirsql")).unwrap();
    fs::write(dir.path().join(".dirsql").join("cache.csv"), "a,b\n1,2").unwrap();

    let matcher = TableMatcher::new(&[("**/*.csv", "t")], &[]).unwrap();
    let results = scan_directory(dir.path(), &matcher);

    assert_eq!(results.len(), 1);
    assert!(results[0].0.ends_with("real.csv"));
}
