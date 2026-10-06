use crate::matcher::{GlobError, Pattern, TableMatcher};
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
    walk(
        root,
        start,
        matcher,
        None,
        false,
        None,
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
/// Skip rules are judged on paths relative to `root`, so a table rooted at a
/// directory the rules would otherwise skip still scans it.
///
/// A dot-named entry is skipped unless `glob` spells it, and symlinks are
/// followed as `bash -O globstar` follows them (see [`PathGlob`]); the walk
/// starts at the path's literal prefix, so a dot directory named there is
/// already inside.
///
/// With `gitignore` set, `.gitignore` files inside a git repo apply as git
/// applies them: hierarchically, each below its own directory, from the repo
/// root (the nearest directory holding `.git`) down, pruning traversal.
/// Outside a repo none applies, as in git, fd and ripgrep. Those above `root`
/// filter what lies below it, never `root` itself, so naming an ignored
/// directory still scans it.
pub fn scan_glob(
    root: &Path,
    glob: &PathGlob,
    ignore: &TableMatcher,
    gitignore: bool,
) -> Vec<PathBuf> {
    let repo = if gitignore {
        enclosing_repo(root, &holds_git)
    } else {
        None
    };
    let repo_frames = repo.map(|top| gitignores_above(root, top));
    let mut results = Vec::new();
    walk(
        root,
        root,
        ignore,
        Some(glob),
        gitignore,
        repo_frames,
        &mut |rel_path, _| {
            if is_wanted(glob, ignore, &rel_path) {
                results.push(rel_path);
            }
        },
    );
    results
}

/// Whether a walked file is a row of the table: it matches the glob and no
/// skip rule ignores it.
fn is_wanted(glob: &PathGlob, ignore: &TableMatcher, rel_path: &Path) -> bool {
    is_glob_match(glob, rel_path) && !ignore.is_ignored(rel_path)
}

/// A path-table's glob as [`scan_glob`] reads it: the files it matches, and
/// the dot-named entries it spells out and so lets the walk into, and each
/// brace-expanded word split into `/`-separated components, which decide
/// where the walk may follow a symlink.
///
/// A component beginning with `.` is spelled; `.claude` and `.env` admit
/// exactly those names, `.*` any dot-named entry, as in the shell. Every
/// other dot-named file or directory is skipped, the rule `ls` and `fd` use.
/// A dot-named symlink is a dot-named entry like any other.
///
/// The symlink rule is bash's (`globstar`, 4.3 and later). A symlinked file
/// is a file. A symlinked directory is entered by any component except `**`,
/// which never traverses one, though it may stop on one for the next
/// component to enter. Each link crossed spends a component, so a cycle
/// cannot recurse without bound.
#[derive(Debug)]
pub struct PathGlob {
    files: Vec<Pattern>,
    spelled_dot_names: Vec<Pattern>,
    /// Every word's components back to back, each word closed by an `End`.
    components: Vec<Component>,
    /// Where each word's components begin.
    starts: Vec<usize>,
}

#[derive(Debug)]
enum Component {
    AnyDepth,
    Name(Pattern),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    File,
    Dir,
    LinkedDir,
}

impl PathGlob {
    fn spells_dot_name(&self, name: &OsStr) -> bool {
        self.spelled_dot_names
            .iter()
            .any(|p| p.is_match(Path::new(name)))
    }

    pub fn is_match(&self, rel_path: &Path) -> bool {
        self.files.iter().any(|p| p.is_match(rel_path))
    }

    fn has_components(&self) -> bool {
        !self.components.is_empty()
    }

    fn start(&self) -> Vec<usize> {
        self.closure(self.starts.clone())
    }

    /// The components that may come next once `name` has been consumed from
    /// `states`; an `End` means a whole word has matched.
    fn step(&self, states: &[usize], name: &OsStr, kind: Kind) -> Vec<usize> {
        let mut next = Vec::new();
        for &i in states {
            match self.components.get(i) {
                Some(Component::AnyDepth) if kind == Kind::LinkedDir => next.push(i + 1),
                Some(Component::AnyDepth) => next.push(i),
                Some(Component::Name(m)) if m.is_match(Path::new(name)) => next.push(i + 1),
                _ => {}
            }
        }
        self.closure(next)
    }

    /// `**` also matches zero names. One pass suffices because repeated `**`
    /// components were collapsed into one.
    fn closure(&self, states: Vec<usize>) -> Vec<usize> {
        let mut states: Vec<usize> = states
            .into_iter()
            .flat_map(|i| {
                let any_depth = matches!(self.components.get(i), Some(Component::AnyDepth));
                std::iter::once(i).chain(any_depth.then_some(i + 1))
            })
            .collect();
        states.sort_unstable();
        states.dedup();
        states
    }

    /// Whether an entry below a followed symlink is worth taking: a file the
    /// whole pattern matches, or a directory some component can still enter.
    fn reaches(&self, states: &[usize], kind: Kind) -> bool {
        let at_end = |&i: &usize| matches!(self.components[i], Component::End);
        if kind == Kind::File {
            states.iter().any(at_end)
        } else {
            !states.iter().all(at_end)
        }
    }
}

/// Compile a single glob pattern into the form [`scan_glob`] expects.
///
/// `literal_separator` is what makes `*` mean *this directory only*: without
/// it a lone `*` would cross `/` and the explicit non-recursive spelling would
/// silently recurse. `**` still crosses separators.
pub fn compile_glob(pattern: &str) -> Result<PathGlob, GlobError> {
    let words = crate::brace::expand(pattern);
    let mut files = Vec::new();
    let mut spelled_dot_names = Vec::new();
    for word in &words {
        files.push(Pattern::new(word)?);
        for component in word.split('/').filter(|c| is_dot_named(OsStr::new(c))) {
            spelled_dot_names.push(Pattern::new(component)?);
        }
    }
    let mut components = Vec::new();
    let mut starts = Vec::new();
    // All or nothing: the walk prunes a directory no component can enter, so
    // a word left without components would lose its matches.
    let compiled: Option<Vec<_>> = words.iter().map(|w| compile_components(w)).collect();
    for word_components in compiled.unwrap_or_default() {
        starts.push(components.len());
        components.extend(word_components);
        components.push(Component::End);
    }
    Ok(PathGlob {
        files,
        spelled_dot_names,
        components,
        starts,
    })
}

fn is_dot_named(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// A word whose components do not compile on their own (a class spanning a
/// `/`) gets none.
fn compile_components(word: &str) -> Option<Vec<Component>> {
    let mut components = word
        .split('/')
        .map(|c| match c {
            "**" => Ok(Component::AnyDepth),
            _ => Pattern::new(c).map(Component::Name),
        })
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    components.dedup_by(|a, b| matches!((a, b), (Component::AnyDepth, Component::AnyDepth)));
    Some(components)
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
/// ignored tree is never read at all. With `glob` given, a dot-named entry it
/// does not spell is skipped; `None` admits them all. With `gitignore` set,
/// entries a `.gitignore` in force ignores are pruned/skipped too, starting
/// from `repo_frames`: the ones in force above `start` when a repo encloses
/// it, `None` when none does. Inside a directory holding `.git`, its
/// `.gitignore` files apply. Symlinks are followed only as `glob` allows, and
/// not at all without one; a broken link or an unreadable directory
/// contributes nothing.
fn walk(
    root: &Path,
    start: &Path,
    ignore: &TableMatcher,
    glob: Option<&PathGlob>,
    gitignore: bool,
    repo_frames: Option<Vec<Gitignore>>,
    visit: &mut dyn FnMut(PathBuf, &DirEntry),
) {
    let rel = start.strip_prefix(root).unwrap_or(start);
    // Depth below `root`, not below `start`: the reserved-directory rule is
    // about the tree's top level wherever the walk begins.
    let depth = rel.components().count();
    let mut walk = Walk {
        ignore,
        glob,
        gitignore,
        in_repo: repo_frames.is_some(),
        frames: repo_frames.unwrap_or_default(),
    };
    let states = glob.map_or_else(Vec::new, PathGlob::start);
    walk.descend(start, rel, depth, &states, false, visit);
}

struct Walk<'a> {
    ignore: &'a TableMatcher,
    glob: Option<&'a PathGlob>,
    gitignore: bool,
    /// Whether a repo encloses the walk's current position, which is what
    /// puts a `.gitignore` in force.
    in_repo: bool,
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
        states: &[usize],
        linked: bool,
        visit: &mut dyn FnMut(PathBuf, &DirEntry),
    ) {
        let entered_repo = self.gitignore && holds_git(dir);
        let was_in_repo = self.in_repo;
        let inherited_frames = entered_repo.then(|| std::mem::take(&mut self.frames));
        self.in_repo |= entered_repo;
        let mut pushed = false;
        if self.in_repo
            && let Some(matcher) = load_gitignore(dir)
        {
            self.frames.push(matcher);
            pushed = true;
        }
        let below = child_depth(depth);
        for (name, entry) in sorted_entries(dir) {
            let Some(kind) = kind_of(&entry, self.glob.is_some()) else {
                continue;
            };
            let next = self.next_states(states, &name, kind);
            let linked = linked || kind == Kind::LinkedDir;
            if !self.follows(linked, &next, kind) {
                continue;
            }
            let is_dir = kind != Kind::File;
            let child = rel.join(&name);
            if self.admits(below, is_dir, &name, &entry.path(), &child) {
                if is_dir {
                    self.descend(&entry.path(), &child, below, &next, linked, visit);
                } else {
                    visit(child, &entry);
                }
            }
        }
        if pushed {
            self.frames.pop();
        }
        if let Some(frames) = inherited_frames {
            self.frames = frames;
        }
        self.in_repo = was_in_repo;
    }

    /// Whether the walk takes an entry at `depth`: the skip rules and the
    /// reserved-directory rule first, then the dot-name rule, then the
    /// `.gitignore` files in force.
    fn admits(&self, depth: usize, is_dir: bool, name: &OsStr, path: &Path, rel: &Path) -> bool {
        should_descend(depth, is_dir, name, rel, self.ignore)
            && self.admits_name(name)
            && (self.frames.is_empty() || !is_gitignored(&self.frames, path, is_dir))
    }

    fn admits_name(&self, name: &OsStr) -> bool {
        match self.glob {
            Some(glob) => !is_dot_named(name) || glob.spells_dot_name(name),
            None => true,
        }
    }

    fn next_states(&self, states: &[usize], name: &OsStr, kind: Kind) -> Vec<usize> {
        self.glob
            .map_or_else(Vec::new, |glob| glob.step(states, name, kind))
    }

    /// Whether the walk takes an entry given whether its path crosses a
    /// followed symlink. Off a link a file is judged on the whole path
    /// afterwards, and a directory is entered only while some component can
    /// still enter it; below one, only a path bash would also reach is taken.
    fn follows(&self, linked: bool, states: &[usize], kind: Kind) -> bool {
        match self.glob {
            Some(glob) if linked || (kind == Kind::Dir && glob.has_components()) => {
                glob.reaches(states, kind)
            }
            Some(_) => true,
            None => !linked,
        }
    }
}

/// What the walk makes of an entry, following a symlink only when
/// `follow_links` is set. `None` for anything else, a broken link included.
fn kind_of(entry: &DirEntry, follow_links: bool) -> Option<Kind> {
    let file_type = entry.file_type().ok()?;
    if !file_type.is_symlink() {
        return classify(file_type.is_dir(), file_type.is_file(), false);
    }
    if !follow_links {
        return None;
    }
    let target = fs::metadata(entry.path()).ok()?;
    classify(target.is_dir(), target.is_file(), true)
}

fn classify(is_dir: bool, is_file: bool, linked: bool) -> Option<Kind> {
    match (is_dir, is_file) {
        (true, _) if linked => Some(Kind::LinkedDir),
        (true, _) => Some(Kind::Dir),
        (_, true) => Some(Kind::File),
        _ => None,
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

/// The `.gitignore` files above `start`, up to and including the repo root
/// `top`, outermost first.
fn gitignores_above(start: &Path, top: &Path) -> Vec<Gitignore> {
    dirs_above(start, top)
        .into_iter()
        .filter_map(load_gitignore)
        .collect()
}

/// The root of the git repo enclosing `start`: the nearest directory, `start`
/// included, holding `.git`.
fn enclosing_repo<'a>(start: &'a Path, is_repo_root: &dyn Fn(&Path) -> bool) -> Option<&'a Path> {
    start.ancestors().find(|dir| is_repo_root(dir))
}

fn holds_git(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// The directories strictly above `start`, up to and including its ancestor
/// `top`, outermost first.
fn dirs_above<'a>(start: &'a Path, top: &Path) -> Vec<&'a Path> {
    let depth = start.ancestors().position(|dir| dir == top).unwrap_or(0);
    let mut dirs: Vec<&Path> = start.ancestors().skip(1).take(depth).collect();
    dirs.reverse();
    dirs
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
fn is_glob_match(glob: &PathGlob, rel_path: &Path) -> bool {
    glob.is_match(rel_path)
}

/// True for the reserved top-level `.dirsql/` directory (`depth == 1`), which
/// the scan unconditionally excludes.
fn is_reserved_dir(depth: usize, is_dir: bool, file_name: &std::ffi::OsStr) -> bool {
    depth == 1 && is_dir && file_name == RESERVED_DIR
}

fn child_depth(parent_depth: usize) -> usize {
    parent_depth + 1
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
    fn child_depth_advances_one_level() {
        assert_eq!(child_depth(3), 4);
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

    #[test]
    fn enclosing_repo_is_the_nearest_directory_holding_git() {
        let repo = enclosing_repo(Path::new("/outer/inner/a"), &|dir| {
            dir == Path::new("/outer") || dir == Path::new("/outer/inner")
        });
        assert_eq!(repo, Some(Path::new("/outer/inner")));
    }

    #[test]
    fn enclosing_repo_includes_the_start_itself() {
        let repo = enclosing_repo(Path::new("/r"), &|dir| dir == Path::new("/r"));
        assert_eq!(repo, Some(Path::new("/r")));
    }

    #[test]
    fn enclosing_repo_is_none_outside_any_repo() {
        assert_eq!(enclosing_repo(Path::new("/idx/docs"), &|_| false), None);
    }

    #[test]
    fn dirs_above_lists_the_ancestors_up_to_top_outermost_first() {
        assert_eq!(
            dirs_above(Path::new("/r/a/b"), Path::new("/r")),
            vec![Path::new("/r"), Path::new("/r/a")]
        );
    }

    #[test]
    fn dirs_above_is_empty_when_the_start_is_the_top() {
        assert!(dirs_above(Path::new("/r"), Path::new("/r")).is_empty());
    }

    #[test]
    fn an_ancestor_frame_filters_entries_below_the_start_but_not_the_start() {
        let frames = vec![frame("/r", &["*.log", "dist/"])];
        assert!(is_gitignored(&frames, Path::new("/r/docs/z.log"), false));
        assert!(!is_gitignored(
            &frames,
            Path::new("/r/dist/bundle.js"),
            false
        ));
    }

    fn walk_with<'a>(
        ignore: &'a TableMatcher,
        glob: Option<&'a PathGlob>,
        frames: Vec<Gitignore>,
    ) -> Walk<'a> {
        Walk {
            ignore,
            glob,
            gitignore: !frames.is_empty(),
            in_repo: !frames.is_empty(),
            frames,
        }
    }

    #[test]
    fn admits_an_ordinary_file_under_no_gitignore() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("**").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
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
        let glob = compile_glob("**").unwrap();
        let walk = walk_with(&ignore, Some(&glob), vec![frame("", &["*.log"])]);
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
        let glob = compile_glob("**").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
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
    fn admits_a_dot_named_entry_the_glob_spells() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("**/.env").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
        assert!(walk.admits(
            2,
            false,
            OsStr::new(".env"),
            Path::new("/r/sub/.env"),
            Path::new("sub/.env")
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
    fn admits_every_dot_named_entry_when_no_glob_governs_the_walk() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, None, Vec::new());
        assert!(walk.admits(
            1,
            false,
            OsStr::new(".env"),
            Path::new("/r/.env"),
            Path::new(".env")
        ));
    }

    #[test]
    fn a_spelled_dot_component_may_itself_be_a_glob() {
        let glob = compile_glob(".env*").unwrap();
        assert!(glob.spells_dot_name(OsStr::new(".env.local")));
        assert!(!glob.spells_dot_name(OsStr::new(".git")));
    }

    #[test]
    fn a_dot_component_is_spelled_wherever_it_sits_in_the_glob() {
        let glob = compile_glob("*/.cache/*").unwrap();
        assert!(glob.spells_dot_name(OsStr::new(".cache")));
        assert!(!glob.spells_dot_name(OsStr::new(".config")));
    }

    #[test]
    fn a_glob_without_a_dot_component_spells_no_dot_name() {
        let glob = compile_glob("docs/**/*.md").unwrap();
        assert!(!glob.spells_dot_name(OsStr::new(".md")));
        assert!(!glob.spells_dot_name(OsStr::new(".docs")));
    }

    #[test]
    fn is_dot_named_looks_at_the_first_byte_only() {
        assert!(is_dot_named(OsStr::new(".env")));
        assert!(is_dot_named(OsStr::new(".")));
        assert!(!is_dot_named(OsStr::new("a.md")));
        assert!(!is_dot_named(OsStr::new("")));
    }

    #[test]
    fn admits_nothing_a_gitignore_in_force_ignores() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("**").unwrap();
        let walk = walk_with(&ignore, Some(&glob), vec![frame("", &["*.log"])]);
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

    fn states_after(glob: &PathGlob, names: &[(&str, Kind)]) -> Vec<usize> {
        names.iter().fold(glob.start(), |states, (name, kind)| {
            glob.step(&states, OsStr::new(name), *kind)
        })
    }

    #[test]
    fn a_double_star_starts_matched_or_not_yet_consumed() {
        assert_eq!(compile_glob("**/*.md").unwrap().start(), vec![0, 1]);
        assert_eq!(compile_glob("*/x").unwrap().start(), vec![0]);
    }

    #[test]
    fn a_name_component_consumes_a_matching_name_of_any_kind() {
        let glob = compile_glob("*/x").unwrap();
        for kind in [Kind::File, Kind::Dir, Kind::LinkedDir] {
            assert_eq!(states_after(&glob, &[("a", kind)]), vec![1]);
        }
        assert_eq!(
            states_after(&glob, &[("a", Kind::Dir), ("y", Kind::File)]),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn a_double_star_stays_on_a_real_directory_or_file() {
        let glob = compile_glob("**/x").unwrap();
        assert_eq!(states_after(&glob, &[("a", Kind::Dir)]), vec![0, 1]);
        assert_eq!(states_after(&glob, &[("a", Kind::File)]), vec![0, 1]);
    }

    #[test]
    fn a_double_star_stops_on_a_symlinked_directory() {
        let glob = compile_glob("**/x").unwrap();
        assert_eq!(states_after(&glob, &[("a", Kind::LinkedDir)]), vec![1]);
        assert_eq!(states_after(&glob, &[("x", Kind::LinkedDir)]), vec![1, 2]);
    }

    #[test]
    fn a_trailing_double_star_matches_files_at_any_depth() {
        let glob = compile_glob("**").unwrap();
        let states = states_after(&glob, &[("a", Kind::Dir), ("b", Kind::File)]);
        assert!(glob.reaches(&states, Kind::File));
    }

    #[test]
    fn repeated_double_stars_are_one() {
        let glob = compile_glob("**/**/x").unwrap();
        assert_eq!(glob.components.len(), 3);
        assert_eq!(states_after(&glob, &[("a", Kind::LinkedDir)]), vec![1]);
    }

    #[test]
    fn a_pattern_whose_components_do_not_compile_alone_follows_no_link() {
        let glob = compile_glob("[a/b]").unwrap();
        assert!(glob.components.is_empty());
        assert!(!glob.reaches(&glob.start(), Kind::LinkedDir));
    }

    #[test]
    fn one_brace_word_without_components_leaves_the_glob_without_any() {
        let glob = compile_glob("{x,[a/b]}").unwrap();
        assert!(glob.components.is_empty());
        assert!(!glob.has_components());
    }

    #[test]
    fn each_brace_word_is_walked_from_its_own_start() {
        let glob = compile_glob("{a,b/c}").unwrap();
        assert!(glob.is_match(Path::new("b/c")));
        assert_eq!(glob.start(), vec![0, 2]);
        assert_eq!(states_after(&glob, &[("b", Kind::LinkedDir)]), vec![3]);
        let states = states_after(&glob, &[("b", Kind::LinkedDir), ("c", Kind::File)]);
        assert!(glob.reaches(&states, Kind::File));
        let states = states_after(&glob, &[("a", Kind::File)]);
        assert!(glob.reaches(&states, Kind::File));
        assert!(!glob.reaches(&states, Kind::LinkedDir));
    }

    #[test]
    fn a_literal_brace_component_matches_only_its_own_text() {
        let glob = compile_glob("{q}/x").unwrap();
        assert_eq!(states_after(&glob, &[("{q}", Kind::LinkedDir)]), vec![1]);
        assert!(states_after(&glob, &[("q", Kind::LinkedDir)]).is_empty());
    }

    #[test]
    fn a_dot_name_inside_a_brace_group_is_spelled() {
        let glob = compile_glob("{.env,x}").unwrap();
        assert!(glob.spells_dot_name(OsStr::new(".env")));
        assert!(!glob.spells_dot_name(OsStr::new(".git")));
    }

    #[test]
    fn reaches_a_file_only_once_the_whole_pattern_has_matched() {
        let glob = compile_glob("*/x").unwrap();
        assert!(glob.reaches(&[2], Kind::File));
        assert!(!glob.reaches(&[1], Kind::File));
    }

    #[test]
    fn reaches_a_directory_only_while_a_component_is_left_to_enter_it() {
        let glob = compile_glob("*/x").unwrap();
        assert!(glob.reaches(&[1], Kind::Dir));
        assert!(glob.reaches(&[1], Kind::LinkedDir));
        assert!(!glob.reaches(&[2], Kind::Dir));
        assert!(!glob.reaches(&[], Kind::LinkedDir));
    }

    #[test]
    fn classify_tells_a_linked_directory_from_a_real_one() {
        assert_eq!(classify(true, false, false), Some(Kind::Dir));
        assert_eq!(classify(true, false, true), Some(Kind::LinkedDir));
        assert_eq!(classify(false, true, false), Some(Kind::File));
        assert_eq!(classify(false, true, true), Some(Kind::File));
        assert_eq!(classify(false, false, true), None);
    }

    #[test]
    fn follows_everything_off_a_link() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, None, Vec::new());
        assert!(walk.follows(false, &[], Kind::File));
        assert!(!walk.follows(true, &[0], Kind::Dir));
    }

    #[test]
    fn follows_below_a_link_only_what_the_glob_reaches() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*/x").unwrap();
        let walk = Walk {
            ignore: &ignore,
            glob: Some(&glob),
            gitignore: false,
            in_repo: false,
            frames: Vec::new(),
        };
        assert!(walk.follows(true, &[2], Kind::File));
        assert!(!walk.follows(true, &[1], Kind::File));
        assert!(walk.follows(false, &[1], Kind::File));
    }

    #[test]
    fn enters_a_real_directory_only_while_a_component_can_enter_it() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*/x").unwrap();
        let walk = Walk {
            ignore: &ignore,
            glob: Some(&glob),
            gitignore: false,
            in_repo: false,
            frames: Vec::new(),
        };
        assert!(walk.follows(false, &[1], Kind::Dir));
        assert!(!walk.follows(false, &[2], Kind::Dir));
        assert!(!walk.follows(false, &[], Kind::Dir));
    }

    #[test]
    fn enters_every_real_directory_when_the_glob_has_no_components() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("[a/b]").unwrap();
        let walk = Walk {
            ignore: &ignore,
            glob: Some(&glob),
            gitignore: false,
            in_repo: false,
            frames: Vec::new(),
        };
        assert!(walk.follows(false, &[], Kind::Dir));
    }

    #[test]
    fn next_states_are_empty_without_a_glob() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let walk = walk_with(&ignore, None, Vec::new());
        assert!(
            walk.next_states(&[0], OsStr::new("a"), Kind::Dir)
                .is_empty()
        );
        let glob = compile_glob("*").unwrap();
        let walk = Walk {
            glob: Some(&glob),
            ..walk
        };
        assert_eq!(walk.next_states(&[0], OsStr::new("a"), Kind::File), vec![1]);
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
