use crate::matcher::TableMatcher;
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

    let walker = WalkDir::new(root).into_iter().filter_entry(|entry| {
        // Skip the reserved `.dirsql/` directory at the top level.
        !is_reserved_dir(entry.depth(), entry.file_type().is_dir(), entry.file_name())
    });

    for entry in walker.filter_map(|e| e.ok()) {
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

        if let Some(table_name) = matcher.match_file(rel_path) {
            results.push((path.to_path_buf(), table_name.to_string()));
        }
    }

    results
}

/// True for the reserved top-level `.dirsql/` directory (`depth == 1`), which
/// the scan unconditionally excludes. Factored out as a pure predicate over the
/// facts `WalkDir` exposes so it can be unit-tested without walking a real tree
/// -- the directory-walk behavior itself is covered by `tests/scanner.rs`.
fn is_reserved_dir(depth: usize, is_dir: bool, file_name: &std::ffi::OsStr) -> bool {
    depth == 1 && is_dir && file_name == RESERVED_DIR
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    // The directory-walk behavior of `scan_directory` (real temp trees, real
    // files) is exercised by `tests/scanner.rs` -- those are integration tests
    // and stay out of this inline unit module, which the `unit lint` isolation
    // rule keeps free of effectful std. What remains here is the pure
    // reserved-dir predicate the walker filters on.

    #[test]
    fn is_reserved_dir_matches_top_level_dirsql() {
        assert!(is_reserved_dir(1, true, OsStr::new(RESERVED_DIR)));
    }

    #[test]
    fn is_reserved_dir_rejects_nested_dirsql() {
        // Only the *top-level* `.dirsql/` (depth 1) is reserved; a nested one
        // (e.g. `sub/.dirsql/`) is an ordinary directory.
        assert!(!is_reserved_dir(2, true, OsStr::new(RESERVED_DIR)));
    }

    #[test]
    fn is_reserved_dir_rejects_files_and_other_names() {
        // A file named `.dirsql` is not the reserved directory ...
        assert!(!is_reserved_dir(1, false, OsStr::new(RESERVED_DIR)));
        // ... and an ordinary top-level directory is not reserved.
        assert!(!is_reserved_dir(1, true, OsStr::new("data")));
    }
}
