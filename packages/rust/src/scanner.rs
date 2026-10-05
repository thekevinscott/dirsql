use crate::matcher::TableMatcher;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::ffi::{OsStr, OsString};
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

/// Top-level directory name reserved for `dirsql`'s own metadata (e.g. the
/// persistent cache database). Always excluded from the scan, regardless of
/// whether persistence is enabled.
pub const RESERVED_DIR: &str = ".dirsql";

/// A root-relative path as dirsql stores and reports it: `/`-separated on
/// every platform, so globs, `file_path` keys and `path` columns agree.
pub(crate) fn to_slash(path: &Path) -> String {
    with_slashes(&path.to_string_lossy(), std::path::MAIN_SEPARATOR)
}

fn with_slashes(path: &str, native: char) -> String {
    if native == '/' {
        return path.to_owned();
    }
    path.replace(native, "/")
}

/// Walk a directory tree and return all file paths paired with their matching table name.
/// Ignored paths and directories are skipped. Only files (not directories) are returned.
///
/// The top-level `.dirsql/` directory is unconditionally excluded.
pub fn scan_directory(root: &Path, matcher: &TableMatcher) -> Vec<(PathBuf, String)> {
    scan_directory_reporting(root, matcher, &mut |_| {})
}

/// [`scan_directory`], calling `on_file` with the running count of files the
/// walk has reached.
///
/// The walk is the one phase of a startup scan with no total to measure
/// against — it does not know how many files there are until it has found them
/// all — so what it can report is a count, not a fraction. See
/// [`crate::progress`], which turns that count into the line a user watching a
/// cold scan sees.
///
/// The count is files *visited*, not the pairs returned: a file matching two
/// tables' globs is one file the walk paid for, and two rows of work for the
/// phase after it.
pub fn scan_directory_reporting(
    root: &Path,
    matcher: &TableMatcher,
    on_file: &mut dyn FnMut(u64),
) -> Vec<(PathBuf, String)> {
    scan_below(root, root, matcher, on_file)
}

/// [`scan_directory`] restricted to the subtree at `dir`, a directory beneath
/// `root`. Paths are still matched and ignored relative to `root`, so the
/// result is exactly the slice of a full scan that falls under `dir`.
///
/// This is how the live watcher indexes a directory that appears whole — a
/// `mkdir` or a rename into the tree — since the OS reports one event for the
/// directory and none for the files already inside it.
pub fn scan_subtree(root: &Path, dir: &Path, matcher: &TableMatcher) -> Vec<(PathBuf, String)> {
    scan_below(root, dir, matcher, &mut |_| {})
}

fn scan_below(
    root: &Path,
    start: &Path,
    matcher: &TableMatcher,
    on_file: &mut dyn FnMut(u64),
) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();
    let mut seen: u64 = 0;

    // Match against relative path so globs like "comments/**/*.jsonl" work
    // regardless of the absolute root directory.
    walk(root, start, matcher, false, &mut |rel_path, entry| {
        if matcher.is_ignored(&rel_path) {
            return;
        }

        seen += 1;
        on_file(seen);

        // Fan-out: a file matching N tables' globs yields N (path, table)
        // pairs, one per matching table, in declaration order.
        for m in matcher.match_all(&rel_path) {
            results.push((entry.path(), m.table_name));
        }
    });

    results
}

/// Walk `root` and return every file whose root-relative path matches `glob`,
/// as root-relative paths in path order.
///
/// The single-glob counterpart to [`scan_directory`]: a path-table names one
/// glob and mints no table names, so there is nothing to fan out over. Shares
/// the walker, and with it the reserved-directory rule, and the same
/// [`TableMatcher`] ignore handling declared tables get.
///
/// Skip rules are judged on paths relative to `root`, so a table rooted at a
/// directory the rules would otherwise skip still scans it.
///
/// With `gitignore` set, `.gitignore` files apply hierarchically (each one
/// below its own directory) and prune traversal, like fd/ripgrep — except
/// that hidden files are still scanned, and no `.git` directory is required.
/// Only files at or below `root` are read; one above it has no say.
pub fn scan_glob(
    root: &Path,
    glob: &GlobSet,
    ignore: &TableMatcher,
    gitignore: bool,
) -> Vec<PathBuf> {
    let mut results = Vec::new();
    walk(root, root, ignore, gitignore, &mut |rel_path, _| {
        if is_wanted(glob, ignore, &rel_path) {
            results.push(rel_path);
        }
    });
    results
}

/// Whether a walked file is a row of the table: it matches the glob and no
/// skip rule ignores it.
fn is_wanted(glob: &GlobSet, ignore: &TableMatcher, rel_path: &Path) -> bool {
    is_glob_match(glob, rel_path) && !ignore.is_ignored(rel_path)
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

/// The shared traversal: every file under `start`, visited with its
/// `root`-relative path, siblings in name order, so the whole walk comes out
/// in path order without a sort at the end. Prunes the reserved top-level
/// `.dirsql/` subtree and any directory the skip rules ignore wholesale, so an
/// ignored tree is never read at all. With `gitignore` set, entries a
/// `.gitignore` in force ignores are pruned/skipped too. Symlinks are not
/// followed; an unreadable directory contributes nothing.
fn walk(
    root: &Path,
    start: &Path,
    ignore: &TableMatcher,
    gitignore: bool,
    visit: &mut dyn FnMut(PathBuf, &DirEntry),
) {
    let rel = start.strip_prefix(root).unwrap_or(start);
    // Depth below `root`, not below `start`: the reserved-directory rule is
    // about the tree's top level wherever the walk begins.
    let depth = rel.components().count();
    let mut walk = Walk {
        ignore,
        gitignore,
        frames: Vec::new(),
    };
    walk.descend(start, rel, depth, visit);
}

struct Walk<'a> {
    ignore: &'a TableMatcher,
    gitignore: bool,
    /// The `.gitignore` files in force at the walk's current position, root
    /// first; a directory's own file is pushed on entry and popped on exit.
    frames: Vec<Gitignore>,
}

impl Walk<'_> {
    fn descend(
        &mut self,
        dir: &Path,
        rel: &Path,
        depth: usize,
        visit: &mut dyn FnMut(PathBuf, &DirEntry),
    ) {
        let mut pushed = false;
        if self.gitignore
            && let Some(matcher) = load_gitignore(dir)
        {
            self.frames.push(matcher);
            pushed = true;
        }
        let below = depth + 1;
        for (name, entry) in sorted_entries(dir) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            let child = rel.join(&name);
            if self.admits(below, is_dir, &name, &entry.path(), &child) {
                if is_dir {
                    self.descend(&entry.path(), &child, below, visit);
                } else if file_type.is_file() {
                    visit(child, &entry);
                }
            }
        }
        if pushed {
            self.frames.pop();
        }
    }

    /// Whether the walk takes an entry at `depth`: the skip rules and the
    /// reserved-directory rule first, then the `.gitignore` files in force.
    fn admits(&self, depth: usize, is_dir: bool, name: &OsStr, path: &Path, rel: &Path) -> bool {
        should_descend(depth, is_dir, name, rel, self.ignore)
            && (self.frames.is_empty() || !is_gitignored(&self.frames, path, is_dir))
    }
}

fn sorted_entries(dir: &Path) -> Vec<(OsString, DirEntry)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(OsString, DirEntry)> = entries
        .filter_map(Result::ok)
        .map(|entry| (entry.file_name(), entry))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
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
fn is_gitignored(frames: &[Gitignore], path: &Path, is_dir: bool) -> bool {
    for frame in frames.iter().rev() {
        match frame.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Whether the walk keeps `rel_path`. False prunes the reserved top-level
/// `.dirsql/` directory and any directory whose whole subtree the skip rules
/// ignore.
fn should_descend(
    depth: usize,
    is_dir: bool,
    file_name: &std::ffi::OsStr,
    rel_path: &Path,
    ignore: &TableMatcher,
) -> bool {
    if is_reserved_dir(depth, is_dir, file_name) {
        return false;
    }
    !is_dir || !ignore.is_ignored_dir(rel_path)
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
    fn with_slashes_rewrites_a_backslash_native_separator() {
        assert_eq!(
            with_slashes(r"moved\one\mid.txt", '\\'),
            "moved/one/mid.txt"
        );
    }

    #[test]
    fn with_slashes_keeps_a_mixed_path_slash_separated() {
        assert_eq!(with_slashes(r"a/b\c.txt", '\\'), "a/b/c.txt");
    }

    #[test]
    fn with_slashes_leaves_a_backslash_in_a_unix_file_name() {
        assert_eq!(with_slashes(r"dir/a\b.txt", '/'), r"dir/a\b.txt");
    }

    #[test]
    fn to_slash_keeps_a_native_unix_path() {
        assert_eq!(to_slash(Path::new("docs/nested/a.md")), "docs/nested/a.md");
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
    fn should_descend_prunes_a_directory_the_skip_rules_fully_ignore() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(!should_descend(
            2,
            true,
            OsStr::new("node_modules"),
            Path::new("apps/node_modules"),
            &ignore
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
            &ignore
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
            &ignore
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
            &ignore
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
    fn frame(dir: &str, lines: &[&str]) -> Gitignore {
        let mut builder = GitignoreBuilder::new(Path::new(dir));
        for line in lines {
            builder.add_line(None, line).unwrap();
        }
        builder.build().unwrap()
    }

    #[test]
    fn is_wanted_takes_a_glob_match_no_skip_rule_ignores() {
        let glob = compile_glob("**/*.md").unwrap();
        let ignore = TableMatcher::new(&[], &["drafts/**"]).unwrap();
        assert!(is_wanted(&glob, &ignore, Path::new("docs/a.md")));
    }

    #[test]
    fn is_wanted_rejects_a_glob_miss_and_an_ignored_match() {
        let glob = compile_glob("**/*.md").unwrap();
        let ignore = TableMatcher::new(&[], &["drafts/**"]).unwrap();
        assert!(!is_wanted(&glob, &ignore, Path::new("docs/a.csv")));
        assert!(!is_wanted(&glob, &ignore, Path::new("drafts/a.md")));
    }

    fn walk_with<'a>(ignore: &'a TableMatcher, frames: Vec<Gitignore>) -> Walk<'a> {
        Walk {
            ignore,
            gitignore: !frames.is_empty(),
            frames,
        }
    }

    #[test]
    fn admits_an_ordinary_file_under_no_gitignore() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, Vec::new());
        assert!(walk.admits(
            1,
            false,
            OsStr::new("a.md"),
            Path::new("/r/a.md"),
            Path::new("a.md")
        ));
    }

    #[test]
    fn admits_nothing_the_skip_rules_prune() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        let walk = walk_with(&ignore, vec![frame("", &["*.log"])]);
        assert!(!walk.admits(
            2,
            true,
            OsStr::new("node_modules"),
            Path::new("/r/apps/node_modules"),
            Path::new("apps/node_modules")
        ));
    }

    #[test]
    fn admits_no_dot_named_entry_the_glob_does_not_spell() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, Vec::new());
        assert!(!walk.admits(
            1,
            false,
            OsStr::new(".env"),
            Path::new("/r/.env"),
            Path::new(".env")
        ));
        assert!(!walk.admits(
            1,
            true,
            OsStr::new(".hidden"),
            Path::new("/r/.hidden"),
            Path::new(".hidden")
        ));
    }

    #[test]
    fn admits_nothing_a_gitignore_in_force_ignores() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, vec![frame("", &["*.log"])]);
        assert!(!walk.admits(
            1,
            false,
            OsStr::new("debug.log"),
            Path::new("/r/debug.log"),
            Path::new("debug.log")
        ));
        assert!(walk.admits(
            1,
            false,
            OsStr::new("app.js"),
            Path::new("/r/app.js"),
            Path::new("app.js")
        ));
    }

    #[test]
    fn is_gitignored_matches_a_rule_from_the_root_gitignore() {
        let frames = [frame("", &["*.log"])];
        assert!(is_gitignored(&frames, Path::new("debug.log"), false));
        assert!(!is_gitignored(&frames, Path::new("app.js"), false));
    }

    #[test]
    fn is_gitignored_marks_a_directory_rule_for_pruning() {
        let frames = [frame("", &["dist/"])];
        assert!(is_gitignored(&frames, Path::new("dist"), true));
    }

    #[test]
    fn is_gitignored_lets_a_deeper_whitelist_override_a_shallower_rule() {
        let frames = [frame("", &["*.log"]), frame("sub", &["!keep.log"])];
        assert!(!is_gitignored(&frames, Path::new("sub/keep.log"), false));
    }

    #[test]
    fn is_gitignored_lets_a_deeper_rule_override_a_shallower_whitelist() {
        let frames = [frame("", &["!keep.log"]), frame("sub", &["*.log"])];
        assert!(is_gitignored(&frames, Path::new("sub/keep.log"), false));
    }
}
