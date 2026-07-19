use crate::matcher::TableMatcher;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Top-level directory name reserved for `dirsql`'s own metadata (e.g. the
/// persistent cache database). Always excluded from the scan, regardless of
/// whether persistence is enabled.
pub const RESERVED_DIR: &str = ".dirsql";

/// Walk a directory tree and return all file paths paired with their matching table name.
/// Ignored paths and directories are skipped. Only files (not directories) are returned.
///
/// The top-level `.dirsql/` directory is unconditionally excluded.
pub fn scan_directory(root: &Path, matcher: &TableMatcher) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();

    for entry in walk(root) {
        let path = entry.path();

        // Match against relative path so globs like "comments/**/*.jsonl" work
        // regardless of the absolute root directory.
        let rel_path = path.strip_prefix(root).unwrap_or(path);

        if matcher.is_ignored(rel_path) {
            continue;
        }

        if !entry.file_type().is_file() {
            continue;
        }

        // Fan-out: a file matching N tables' globs yields N (path, table)
        // pairs, one per matching table, in declaration order.
        for m in matcher.match_all(rel_path) {
            results.push((path.to_path_buf(), m.table_name));
        }
    }

    results
}

/// Walk `root` and return every file whose root-relative path matches `glob`,
/// as root-relative paths in sorted order.
///
/// The single-glob counterpart to [`scan_directory`]: a path-table names one
/// glob and mints no table names, so there is nothing to fan out over. Shares
/// the walker, and with it the reserved-directory rule.
pub fn scan_glob(root: &Path, glob: &GlobSet) -> Vec<PathBuf> {
    let mut results = Vec::new();

    for entry in walk(root) {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel_path = path.strip_prefix(root).unwrap_or(path);

        if is_glob_match(glob, rel_path) {
            results.push(rel_path.to_path_buf());
        }
    }

    // Stable ordering so a path-table scan is reproducible across runs;
    // walkdir's traversal order is filesystem-dependent.
    results.sort();
    results
}

/// Compile a single glob pattern into the set [`scan_glob`] expects.
pub fn compile_glob(pattern: &str) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    builder.add(Glob::new(pattern)?);
    builder.build()
}

/// The shared traversal: every entry under `root` with the reserved top-level
/// `.dirsql/` subtree pruned.
fn walk(root: &Path) -> impl Iterator<Item = walkdir::DirEntry> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| {
            !is_reserved_dir(entry.depth(), entry.file_type().is_dir(), entry.file_name())
        })
        .filter_map(Result::ok)
}

/// Whether `rel_path` matches `glob`.
fn is_glob_match(glob: &GlobSet, rel_path: &Path) -> bool {
    glob.is_match(rel_path)
}

/// True for the reserved top-level `.dirsql/` directory (`depth == 1`), which
/// the scan unconditionally excludes.
fn is_reserved_dir(depth: usize, is_dir: bool, file_name: &std::ffi::OsStr) -> bool {
    depth == 1 && is_dir && file_name == RESERVED_DIR
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    // Real directory-walk behavior is covered by `tests/scanner.rs`
    // (unit-lint isolation); only the pure predicate is tested here.

    #[test]
    fn is_reserved_dir_matches_top_level_dirsql() {
        assert!(is_reserved_dir(1, true, OsStr::new(RESERVED_DIR)));
    }

    #[test]
    fn is_reserved_dir_rejects_nested_dirsql() {
        assert!(!is_reserved_dir(2, true, OsStr::new(RESERVED_DIR)));
    }

    #[test]
    fn is_reserved_dir_rejects_files_and_other_names() {
        assert!(!is_reserved_dir(1, false, OsStr::new(RESERVED_DIR)));
        assert!(!is_reserved_dir(1, true, OsStr::new("data")));
    }

    #[test]
    fn is_glob_match_accepts_a_matching_relative_path() {
        let set = compile_glob("docs/**/*.md").unwrap();
        assert!(is_glob_match(&set, Path::new("docs/a.md")));
    }

    #[test]
    fn is_glob_match_rejects_a_non_matching_relative_path() {
        let set = compile_glob("docs/**/*.md").unwrap();
        assert!(!is_glob_match(&set, Path::new("docs/a.csv")));
    }

    #[test]
    fn is_glob_match_is_scoped_to_the_pattern_prefix() {
        let set = compile_glob("docs/**/*.md").unwrap();
        assert!(!is_glob_match(&set, Path::new("a.md")));
    }

    #[test]
    fn compile_glob_rejects_an_invalid_pattern() {
        assert!(compile_glob("[").is_err());
    }
}
