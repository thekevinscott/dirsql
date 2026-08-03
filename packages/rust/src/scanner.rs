use crate::matcher::TableMatcher;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
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

    for entry in walk(root, matcher, Path::new("")) {
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
/// the walker, and with it the reserved-directory rule, and the same
/// [`TableMatcher`] ignore handling declared tables get.
///
/// Skip rules are evaluated against the path *below* `ignore_base` — the
/// literal directories the pattern named outright — so a path that reaches
/// into an ignored directory on purpose still scans it.
pub fn scan_glob(
    root: &Path,
    glob: &GlobSet,
    ignore: &TableMatcher,
    ignore_base: &Path,
) -> Vec<PathBuf> {
    let mut results = Vec::new();

    for entry in walk(root, ignore, ignore_base) {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel_path = path.strip_prefix(root).unwrap_or(path);

        if is_glob_match(glob, rel_path) && !is_ignored_below(ignore, ignore_base, rel_path) {
            results.push(rel_path.to_path_buf());
        }
    }

    // Stable ordering so a path-table scan is reproducible across runs;
    // walkdir's traversal order is filesystem-dependent.
    results.sort();
    results
}

/// Compile a single glob pattern into the set [`scan_glob`] expects.
///
/// `literal_separator` is what makes `*` mean *this directory only*: without
/// it a lone `*` would cross `/` and the explicit non-recursive spelling would
/// silently recurse. `**` still crosses separators.
pub fn compile_glob(pattern: &str) -> Result<GlobSet, globset::Error> {
    let mut builder = GlobSetBuilder::new();
    builder.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
    builder.build()
}

/// The shared traversal: every entry under `root`, pruning the reserved
/// top-level `.dirsql/` subtree and any directory the skip rules ignore
/// wholesale, so an ignored tree is never read at all.
fn walk<'a>(
    root: &Path,
    ignore: &'a TableMatcher,
    ignore_base: &'a Path,
) -> impl Iterator<Item = walkdir::DirEntry> + 'a {
    let root = root.to_path_buf();
    WalkDir::new(&root)
        .into_iter()
        .filter_entry(move |entry| {
            let rel_path = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            should_descend(
                entry.depth(),
                entry.file_type().is_dir(),
                entry.file_name(),
                rel_path,
                ignore,
                ignore_base,
            )
        })
        .filter_map(Result::ok)
}

/// Whether the walk keeps `rel_path`. False prunes the reserved top-level
/// `.dirsql/` directory and any directory whose whole subtree the skip rules
/// ignore — unless the literal base the pattern named runs through it, which
/// keeps a scan pointed *into* an ignored directory working.
fn should_descend(
    depth: usize,
    is_dir: bool,
    file_name: &std::ffi::OsStr,
    rel_path: &Path,
    ignore: &TableMatcher,
    ignore_base: &Path,
) -> bool {
    if is_reserved_dir(depth, is_dir, file_name) {
        return false;
    }
    if !is_dir || ignore_base.starts_with(rel_path) {
        return true;
    }
    let below = rel_path.strip_prefix(ignore_base).unwrap_or(rel_path);
    !ignore.is_ignored_dir(below)
}

/// Whether `rel_path` matches `glob`.
fn is_glob_match(glob: &GlobSet, rel_path: &Path) -> bool {
    glob.is_match(rel_path)
}

/// Whether `rel_path` is ignored, judged on the part of it beneath `base`.
fn is_ignored_below(ignore: &TableMatcher, base: &Path, rel_path: &Path) -> bool {
    let below = rel_path.strip_prefix(base).unwrap_or(rel_path);
    ignore.is_ignored(below)
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

    #[test]
    fn a_single_star_does_not_cross_a_directory_separator() {
        let set = compile_glob("*").unwrap();
        assert!(is_glob_match(&set, Path::new("a.md")));
        assert!(!is_glob_match(&set, Path::new("docs/a.md")));
    }

    #[test]
    fn a_double_star_still_crosses_directory_separators() {
        let set = compile_glob("**/*").unwrap();
        assert!(is_glob_match(&set, Path::new("a.md")));
        assert!(is_glob_match(&set, Path::new("docs/deep/a.md")));
    }

    #[test]
    fn is_ignored_below_matches_an_ignored_path_at_the_top() {
        let ignore = TableMatcher::new(&[], &["node_modules/**"]).unwrap();
        assert!(is_ignored_below(
            &ignore,
            Path::new(""),
            Path::new("node_modules/pkg/index.js")
        ));
    }

    #[test]
    fn is_ignored_below_exempts_the_base_the_pattern_named() {
        let ignore = TableMatcher::new(&[], &["node_modules/**"]).unwrap();
        assert!(!is_ignored_below(
            &ignore,
            Path::new("node_modules"),
            Path::new("node_modules/pkg/index.js")
        ));
    }

    #[test]
    fn is_ignored_below_judges_the_whole_path_when_the_base_does_not_apply() {
        let ignore = TableMatcher::new(&[], &["other/**"]).unwrap();
        assert!(is_ignored_below(
            &ignore,
            Path::new("docs"),
            Path::new("other/a.tmp")
        ));
    }

    #[test]
    fn is_ignored_below_passes_an_unignored_path() {
        let ignore = TableMatcher::new(&[], &["node_modules/**"]).unwrap();
        assert!(!is_ignored_below(
            &ignore,
            Path::new(""),
            Path::new("docs/a.md")
        ));
    }

    #[test]
    fn should_descend_prunes_a_directory_the_skip_rules_fully_ignore() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(!should_descend(
            2,
            true,
            OsStr::new("node_modules"),
            Path::new("apps/node_modules"),
            &ignore,
            Path::new("")
        ));
    }

    #[test]
    fn should_descend_keeps_an_ordinary_directory() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(
            1,
            true,
            OsStr::new("docs"),
            Path::new("docs"),
            &ignore,
            Path::new("")
        ));
    }

    #[test]
    fn should_descend_keeps_a_file_even_when_a_subtree_pattern_names_it() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(
            2,
            false,
            OsStr::new("node_modules"),
            Path::new("apps/node_modules"),
            &ignore,
            Path::new("")
        ));
    }

    #[test]
    fn should_descend_keeps_the_directory_the_base_names() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(
            1,
            true,
            OsStr::new("node_modules"),
            Path::new("node_modules"),
            &ignore,
            Path::new("node_modules")
        ));
    }

    #[test]
    fn should_descend_keeps_an_ancestor_of_the_named_base() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(
            2,
            true,
            OsStr::new("node_modules"),
            Path::new("apps/node_modules"),
            &ignore,
            Path::new("apps/node_modules/pkg")
        ));
    }

    #[test]
    fn should_descend_judges_the_part_below_the_named_base() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(!should_descend(
            3,
            true,
            OsStr::new("node_modules"),
            Path::new("apps/pkg/node_modules"),
            &ignore,
            Path::new("apps")
        ));
    }

    #[test]
    fn should_descend_prunes_the_reserved_top_level_dirsql_directory() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        assert!(!should_descend(
            1,
            true,
            OsStr::new(RESERVED_DIR),
            Path::new(RESERVED_DIR),
            &ignore,
            Path::new("")
        ));
    }
}
