use crate::matcher::TableMatcher;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
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

    for entry in walk(root, matcher, Path::new(""), false) {
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
///
/// With `gitignore` set, `.gitignore` files apply hierarchically (each one
/// below its own directory) and prune traversal, like fd/ripgrep — except
/// that hidden files are still scanned, and no `.git` directory is required.
/// Rules from `.gitignore` files *above* `ignore_base` are exempt beneath it,
/// mirroring the skip-rule exemption.
pub fn scan_glob(
    root: &Path,
    glob: &GlobSet,
    ignore: &TableMatcher,
    ignore_base: &Path,
    gitignore: bool,
) -> Vec<PathBuf> {
    let mut results = Vec::new();

    for entry in walk(root, ignore, ignore_base, gitignore) {
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

/// Module-argument spelling for a scan that respects `.gitignore`.
pub(crate) const GITIGNORE_ARG: &str = "gitignore";

/// Module-argument spelling for a scan that does not (the CLI's `--no-ignore`).
pub(crate) const NO_GITIGNORE_ARG: &str = "no-gitignore";

/// Parse the gitignore module argument both path-table modules take.
pub(crate) fn parse_gitignore_arg(arg: &str) -> Result<bool, String> {
    match arg {
        GITIGNORE_ARG => Ok(true),
        NO_GITIGNORE_ARG => Ok(false),
        other => Err(format!(
            "expected '{GITIGNORE_ARG}' or '{NO_GITIGNORE_ARG}', got {other:?}"
        )),
    }
}

/// The shared traversal: every entry under `root`, pruning the reserved
/// top-level `.dirsql/` subtree and any directory the skip rules ignore
/// wholesale, so an ignored tree is never read at all. With `gitignore` set,
/// entries a `.gitignore` in force ignores are pruned/skipped too.
fn walk<'a>(
    root: &Path,
    ignore: &'a TableMatcher,
    ignore_base: &'a Path,
    gitignore: bool,
) -> impl Iterator<Item = walkdir::DirEntry> + 'a {
    let root = root.to_path_buf();
    // The `.gitignore` files in force at the walk's current position, root
    // first. `filter_entry` visits a directory before its children and the
    // children before the next sibling, so a depth-keyed stack tracks scope.
    let mut frames: Vec<GitignoreFrame> = Vec::new();
    WalkDir::new(&root)
        .into_iter()
        .filter_entry(move |entry| {
            let rel_path = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            let is_dir = entry.file_type().is_dir();
            if !should_descend(
                entry.depth(),
                is_dir,
                entry.file_name(),
                rel_path,
                ignore,
                ignore_base,
            ) {
                return false;
            }
            if !gitignore {
                return true;
            }
            while frames.last().is_some_and(|f| f.depth >= entry.depth()) {
                frames.pop();
            }
            if is_gitignored(&frames, entry.path(), rel_path, is_dir, ignore_base) {
                return false;
            }
            if is_dir && let Some(matcher) = load_gitignore(entry.path()) {
                frames.push(GitignoreFrame {
                    depth: entry.depth(),
                    dir: rel_path.to_path_buf(),
                    matcher,
                });
            }
            true
        })
        .filter_map(Result::ok)
}

/// One `.gitignore` in force over the walk: its compiled matcher, the
/// root-relative directory it sits in, and that directory's walk depth.
struct GitignoreFrame {
    depth: usize,
    dir: PathBuf,
    matcher: Gitignore,
}

/// Compile the `.gitignore` in `dir`, if one exists. An unparsable file
/// degrades to no matcher rather than failing the scan.
fn load_gitignore(dir: &Path) -> Option<Gitignore> {
    let file = dir.join(".gitignore");
    if !file.is_file() {
        return None;
    }
    let mut builder = GitignoreBuilder::new(dir);
    builder.add(&file);
    builder.build().ok()
}

/// Whether the `.gitignore` files in force mark `path` ignored. Deeper files
/// take precedence (git's rule), and a whitelisting `!pattern` un-ignores.
/// The literal chain the scan's pattern named outright is exempt, as are
/// rules from files *above* [`ignore_base`] for entries beneath it — naming a
/// gitignored directory on purpose still scans it.
fn is_gitignored(
    frames: &[GitignoreFrame],
    path: &Path,
    rel_path: &Path,
    is_dir: bool,
    ignore_base: &Path,
) -> bool {
    if ignore_base.starts_with(rel_path) {
        return false;
    }
    for frame in frames.iter().rev() {
        if !frame_applies(&frame.dir, rel_path, ignore_base) {
            continue;
        }
        match frame.matcher.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Whether a `.gitignore` living at `frame_dir` gets a say over `rel_path`.
/// False exactly when the entry sits inside `ignore_base` and the file sits
/// strictly above it: the base was named outright, so ancestors' rules do not
/// reach past it, while a `.gitignore` at or below the base still applies.
fn frame_applies(frame_dir: &Path, rel_path: &Path, ignore_base: &Path) -> bool {
    let entry_inside_base =
        !ignore_base.as_os_str().is_empty() && rel_path.starts_with(ignore_base);
    let frame_above_base = frame_dir != ignore_base && ignore_base.starts_with(frame_dir);
    !(entry_inside_base && frame_above_base)
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

    #[test]
    fn parse_gitignore_arg_accepts_both_spellings() {
        assert_eq!(parse_gitignore_arg(GITIGNORE_ARG), Ok(true));
        assert_eq!(parse_gitignore_arg(NO_GITIGNORE_ARG), Ok(false));
    }

    #[test]
    fn parse_gitignore_arg_rejects_anything_else_naming_both_spellings() {
        let err = parse_gitignore_arg("maybe").unwrap_err();
        assert!(err.contains("'gitignore'"), "got: {err}");
        assert!(err.contains("'no-gitignore'"), "got: {err}");
        assert!(err.contains("maybe"), "got: {err}");
    }

    /// A gitignore frame compiled from in-memory lines; no filesystem.
    fn frame(depth: usize, dir: &str, lines: &[&str]) -> GitignoreFrame {
        let mut builder = GitignoreBuilder::new(Path::new(dir));
        for line in lines {
            builder.add_line(None, line).unwrap();
        }
        GitignoreFrame {
            depth,
            dir: PathBuf::from(dir),
            matcher: builder.build().unwrap(),
        }
    }

    #[test]
    fn is_gitignored_matches_a_rule_from_the_root_gitignore() {
        let frames = [frame(0, "", &["*.log"])];
        assert!(is_gitignored(
            &frames,
            Path::new("debug.log"),
            Path::new("debug.log"),
            false,
            Path::new("")
        ));
        assert!(!is_gitignored(
            &frames,
            Path::new("app.js"),
            Path::new("app.js"),
            false,
            Path::new("")
        ));
    }

    #[test]
    fn is_gitignored_marks_a_directory_rule_for_pruning() {
        let frames = [frame(0, "", &["dist/"])];
        assert!(is_gitignored(
            &frames,
            Path::new("dist"),
            Path::new("dist"),
            true,
            Path::new("")
        ));
    }

    #[test]
    fn is_gitignored_lets_a_deeper_whitelist_override_a_shallower_rule() {
        let frames = [frame(0, "", &["*.log"]), frame(1, "sub", &["!keep.log"])];
        assert!(!is_gitignored(
            &frames,
            Path::new("sub/keep.log"),
            Path::new("sub/keep.log"),
            false,
            Path::new("")
        ));
    }

    #[test]
    fn is_gitignored_exempts_the_literal_chain_the_pattern_named() {
        let frames = [frame(0, "", &["dist/"])];
        assert!(!is_gitignored(
            &frames,
            Path::new("dist"),
            Path::new("dist"),
            true,
            Path::new("dist")
        ));
    }

    #[test]
    fn is_gitignored_exempts_ancestor_rules_beneath_the_named_base() {
        let frames = [frame(0, "", &["dist/pkg/"])];
        assert!(!is_gitignored(
            &frames,
            Path::new("dist/pkg"),
            Path::new("dist/pkg"),
            true,
            Path::new("dist")
        ));
    }

    #[test]
    fn is_gitignored_honors_a_rule_at_the_named_base_itself() {
        let frames = [frame(1, "dist", &["*.map"])];
        assert!(is_gitignored(
            &frames,
            Path::new("dist/a.map"),
            Path::new("dist/a.map"),
            false,
            Path::new("dist")
        ));
    }

    #[test]
    fn frame_applies_everywhere_with_no_named_base() {
        assert!(frame_applies(
            Path::new(""),
            Path::new("a.md"),
            Path::new("")
        ));
    }

    #[test]
    fn frame_applies_to_entries_outside_the_named_base() {
        assert!(frame_applies(
            Path::new(""),
            Path::new("other/a.md"),
            Path::new("dist")
        ));
    }

    #[test]
    fn frame_above_the_base_does_not_apply_beneath_it() {
        assert!(!frame_applies(
            Path::new(""),
            Path::new("dist/a.js"),
            Path::new("dist")
        ));
    }

    #[test]
    fn frame_at_the_base_still_applies_beneath_it() {
        assert!(frame_applies(
            Path::new("dist"),
            Path::new("dist/a.js"),
            Path::new("dist")
        ));
    }

    #[test]
    fn frame_below_the_base_still_applies_beneath_it() {
        assert!(frame_applies(
            Path::new("dist/sub"),
            Path::new("dist/sub/a.js"),
            Path::new("dist")
        ));
    }
}
