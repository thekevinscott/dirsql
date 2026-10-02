use crate::matcher::TableMatcher;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::ffi::OsString;
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
    walk(
        root,
        start,
        matcher,
        Path::new(""),
        false,
        &mut |rel_path, entry| {
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
        },
    );

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
    walk(
        root,
        root,
        ignore,
        ignore_base,
        gitignore,
        &mut |rel_path, _| {
            if is_glob_match(glob, &rel_path) && !is_ignored_below(ignore, ignore_base, &rel_path) {
                results.push(rel_path);
            }
        },
    );
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
    ignore_base: &Path,
    gitignore: bool,
    visit: &mut dyn FnMut(PathBuf, &DirEntry),
) {
    let rel = start.strip_prefix(root).unwrap_or(start);
    // Depth below `root`, not below `start`: the reserved-directory rule is
    // about the tree's top level wherever the walk begins.
    let depth = rel.components().count();
    let mut walk = Walk {
        ignore,
        ignore_base,
        gitignore,
        frames: Vec::new(),
    };
    walk.descend(start, rel, depth, visit);
}

struct Walk<'a> {
    ignore: &'a TableMatcher,
    ignore_base: &'a Path,
    gitignore: bool,
    /// The `.gitignore` files in force at the walk's current position, root
    /// first; a directory's own file is pushed on entry and popped on exit.
    frames: Vec<GitignoreFrame>,
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
            self.frames.push(GitignoreFrame {
                dir: rel.to_path_buf(),
                matcher,
            });
            pushed = true;
        }
        for (name, entry) in sorted_entries(dir) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            let child = rel.join(&name);
            if !should_descend(
                depth + 1,
                is_dir,
                &name,
                &child,
                self.ignore,
                self.ignore_base,
            ) {
                continue;
            }
            if !self.frames.is_empty()
                && is_gitignored(
                    &self.frames,
                    &entry.path(),
                    &child,
                    is_dir,
                    self.ignore_base,
                )
            {
                continue;
            }
            if is_dir {
                self.descend(&entry.path(), &child, depth + 1, visit);
            } else if file_type.is_file() {
                visit(child, &entry);
            }
        }
        if pushed {
            self.frames.pop();
        }
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

/// One `.gitignore` in force over the walk: its compiled matcher and the
/// root-relative directory it sits in.
struct GitignoreFrame {
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
    fn frame(dir: &str, lines: &[&str]) -> GitignoreFrame {
        let mut builder = GitignoreBuilder::new(Path::new(dir));
        for line in lines {
            builder.add_line(None, line).unwrap();
        }
        GitignoreFrame {
            dir: PathBuf::from(dir),
            matcher: builder.build().unwrap(),
        }
    }

    #[test]
    fn is_gitignored_matches_a_rule_from_the_root_gitignore() {
        let frames = [frame("", &["*.log"])];
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
        let frames = [frame("", &["dist/"])];
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
        let frames = [frame("", &["*.log"]), frame("sub", &["!keep.log"])];
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
        let frames = [frame("", &["dist/"])];
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
        let frames = [frame("", &["dist/pkg/"])];
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
        let frames = [frame("dist", &["*.map"])];
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
