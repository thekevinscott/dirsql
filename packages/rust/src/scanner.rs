use crate::listing::{Seen, read_judged};
use crate::matcher::{GlobError, Pattern, TableMatcher};
use crate::tree_walk::{Step, walk_in_order};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

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

/// `start` and every directory below it that [`scan_subtree`] would enter,
/// judged relative to `root`. These are the directories whose entries can
/// change what the index holds.
pub fn scan_dirs(root: &Path, start: &Path, ignore: &TableMatcher) -> Vec<PathBuf> {
    let walker = Walk {
        ignore,
        glob: None,
        gitignore: false,
        in_repo: false,
        frames: Vec::new(),
        dirs: true,
    };
    let mut dirs = vec![start.to_path_buf()];
    walk(root, start, walker, &|_| false, &mut |rel_path, parent| {
        dirs.push(parent.join(rel_path.file_name().unwrap_or_default()));
    });
    dirs
}

fn scan_below(
    root: &Path,
    start: &Path,
    matcher: &TableMatcher,
    on_file: &mut dyn FnMut(u64),
) -> Vec<(PathBuf, String)> {
    let mut results = Vec::new();
    let mut seen: u64 = 0;
    let gitignore = matcher.respects_gitignore();
    let repo = if gitignore {
        enclosing_repo(start, &holds_git)
    } else {
        None
    };

    // Match against relative path so globs like "comments/**/*.jsonl" work
    // regardless of the absolute root directory.
    let walker = Walk {
        ignore: matcher,
        glob: Some(matcher.walk_glob()),
        gitignore,
        in_repo: repo.is_some(),
        frames: repo.map_or_else(Vec::new, |top| {
            gitignores_above(start, top, &load_gitignore)
        }),
        dirs: false,
    };
    walk(
        root,
        start,
        walker,
        &|rel_path| !matcher.is_ignored(rel_path),
        &mut |rel_path, dir| {
            seen += 1;
            on_file(seen);

            // Fan-out: a file matching N tables' globs yields N (path, table)
            // pairs, one per matching table, in declaration order.
            let path = dir.join(rel_path.file_name().unwrap_or_default());
            for m in matcher.match_all(&rel_path) {
                results.push((path.clone(), m.table_name));
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
/// the walker and the same [`TableMatcher`] ignore handling declared tables get.
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
/// directory still scans it. [`scan_glob_checking_root`] lifts that for a
/// root that is only the literal prefix of a wildcard.
pub fn scan_glob(
    root: &Path,
    glob: &PathGlob,
    ignore: &TableMatcher,
    gitignore: bool,
) -> Vec<PathBuf> {
    scan_glob_checking_root(root, glob, ignore, gitignore, false)
}

/// [`scan_glob`], and with `check_root` set a `root` that `.gitignore` ignores
/// yields nothing: for a glob whose wildcard sits below a literal prefix, the
/// prefix is not a path the pattern spells out.
pub fn scan_glob_checking_root(
    root: &Path,
    glob: &PathGlob,
    ignore: &TableMatcher,
    gitignore: bool,
    check_root: bool,
) -> Vec<PathBuf> {
    if gitignore && check_root && !holds_git(root) && is_gitignored_path(root, true) {
        return Vec::new();
    }
    let repo = if gitignore {
        enclosing_repo(root, &holds_git)
    } else {
        None
    };
    let walker = Walk {
        ignore,
        glob: Some(glob),
        gitignore,
        in_repo: repo.is_some(),
        frames: repo.map_or_else(Vec::new, |top| gitignores_above(root, top, &load_gitignore)),
        dirs: false,
    };
    let mut results = Vec::new();
    walk(
        root,
        root,
        walker,
        &|rel_path| is_wanted(glob, ignore, rel_path),
        &mut |rel_path, _| results.push(rel_path),
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
/// exactly those names, `.*` any dot-named entry, as in the shell, and only at
/// the depth the component sits: `**` never consumes a dot-named directory.
/// Every other dot-named file or directory is skipped, the rule `ls` and `fd`
/// use.
/// A dot-named symlink is a dot-named entry like any other.
///
/// The symlink rule is bash's (`globstar`, 4.3 and later). A symlinked file
/// is a file. A symlinked directory is entered by any component except `**`,
/// which never traverses one, though it may stop on one for the next
/// component to enter. Each link crossed spends a component, so a cycle
/// cannot recurse without bound.
#[derive(Debug, Clone)]
pub struct PathGlob {
    files: Vec<Pattern>,
    spelled_dot_names: Vec<Pattern>,
    /// Every word's components back to back, each word closed by an `End`.
    components: Vec<Component>,
    /// Where each word's components begin.
    starts: Vec<usize>,
    /// Whether any word spells a `.` or `..` component, which the walk has to
    /// enter itself: a directory listing never names them.
    spells_relative: bool,
    /// The words that spell a path outright, with no wildcard.
    named: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
enum Component {
    AnyDepth,
    Name(Pattern),
    /// A component spelled with a leading `.`, the only kind that consumes a
    /// dot-named entry.
    DotName(Pattern),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    File,
    Dir,
    LinkedDir,
}

impl PathGlob {
    pub(crate) fn spells_dot_name(&self, name: &OsStr) -> bool {
        self.spelled_dot_names
            .iter()
            .any(|p| p.is_match(Path::new(name)))
    }

    /// Whether `rel` is a path a wildcard-free word spells, or a directory on
    /// the way to one.
    /// `self` that also takes each of `dirs` as a path it spells out.
    pub(crate) fn with_named_dirs(mut self, dirs: &[String]) -> Self {
        self.named.extend(dirs.iter().map(PathBuf::from));
        self
    }

    fn names(&self, rel: &Path) -> bool {
        self.named.iter().any(|word| word.starts_with(rel))
    }

    pub fn is_match(&self, rel_path: &Path) -> bool {
        self.files.iter().any(|p| p.is_match(rel_path))
    }

    /// [`is_match`](Self::is_match), and every dot-named component of the path
    /// is one the glob spells.
    pub(crate) fn is_match_unhidden(&self, rel_path: &Path) -> bool {
        self.is_match(rel_path)
            && rel_path
                .components()
                .map(|c| c.as_os_str())
                .filter(|name| is_dot_named(name))
                .all(|name| self.spells_dot_name(name))
    }

    fn has_components(&self) -> bool {
        !self.components.is_empty()
    }

    /// The `.` and `..` entries a directory reached with `states` has as far
    /// as the pattern is concerned, each with the states it leads to.
    fn relative_entries(&self, states: &[usize]) -> Vec<(&'static str, Vec<usize>)> {
        if !self.spells_relative {
            return Vec::new();
        }
        [".", ".."]
            .into_iter()
            .map(|name| (name, self.step(states, OsStr::new(name), Kind::Dir)))
            .filter(|(_, next)| self.reaches(next, Kind::Dir))
            .collect()
    }

    fn start(&self) -> Vec<usize> {
        self.closure(self.starts.clone())
    }

    /// The components that may come next once `name` has been consumed from
    /// `states`; an `End` means a whole word has matched.
    fn step(&self, states: &[usize], name: &OsStr, kind: Kind) -> Vec<usize> {
        let mut next = Vec::new();
        let dot_named = is_dot_named(name);
        for &i in states {
            match self.components.get(i) {
                Some(Component::AnyDepth) if dot_named => {}
                Some(Component::AnyDepth) if kind == Kind::LinkedDir => next.push(i + 1),
                Some(Component::AnyDepth) => next.push(i),
                Some(Component::Name(m)) if !dot_named && m.is_match(Path::new(name)) => {
                    next.push(i + 1)
                }
                Some(Component::DotName(m)) if m.is_match(Path::new(name)) => next.push(i + 1),
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
    compile_globs(&[pattern])
}

/// One [`PathGlob`] over every word of every pattern: the walk a set of
/// tables shares, entering what any of them could reach.
pub fn compile_globs(patterns: &[&str]) -> Result<PathGlob, GlobError> {
    let words: Vec<String> = patterns
        .iter()
        .flat_map(|pattern| crate::brace::expand(pattern))
        .map(|word| collapse_separators(&word))
        .collect();
    let spells_relative = words
        .iter()
        .any(|word| word.split('/').any(|c| c == "." || c == ".."));
    let mut files = Vec::new();
    let mut spelled_dot_names = Vec::new();
    for word in &words {
        files.push(Pattern::new(word)?);
        for component in word.split('/').filter(|c| is_dot_named(OsStr::new(c))) {
            spelled_dot_names.push(Pattern::new(component)?);
        }
    }
    let named = words
        .iter()
        .filter(|word| !word.contains(['*', '?', '[', '\\']))
        .map(PathBuf::from)
        .collect();
    let mut components = Vec::new();
    let mut starts = Vec::new();
    for word in &words {
        let word_components = compile_components(word)?;
        starts.push(components.len());
        components.extend(word_components);
        components.push(Component::End);
    }
    Ok(PathGlob {
        files,
        spelled_dot_names,
        components,
        starts,
        spells_relative,
        named,
    })
}

/// `word` with each run of `/` made one, as the shell reads a pattern's
/// components.
fn collapse_separators(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if c != '/' || !out.ends_with('/') {
            out.push(c);
        }
    }
    out
}

fn is_dot_named(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

fn compile_components(word: &str) -> Result<Vec<Component>, GlobError> {
    let mut components = word
        .split('/')
        .map(|c| match c {
            "**" => Ok(Component::AnyDepth),
            _ if is_dot_named(OsStr::new(c)) => Pattern::new(c).map(Component::DotName),
            _ => Pattern::new(c).map(Component::Name),
        })
        .collect::<Result<Vec<_>, _>>()?;
    components.dedup_by(|a, b| matches!((a, b), (Component::AnyDepth, Component::AnyDepth)));
    Ok(components)
}

/// Module-argument spelling for a scan that respects `.gitignore`.
pub(crate) const GITIGNORE_ARG: &str = "gitignore";

/// Module-argument spelling for a scan that does not (the CLI's `--no-ignore`).
pub(crate) const NO_GITIGNORE_ARG: &str = "no-gitignore";

/// Module-argument spelling for a scan that respects `.gitignore` files and
/// also applies them to its own root.
pub(crate) const GITIGNORE_ROOT_ARG: &str = "gitignore-root";

/// Parse the gitignore module argument both path-table modules take.
pub(crate) fn parse_gitignore_arg(arg: &str) -> Result<bool, String> {
    match arg {
        GITIGNORE_ARG | GITIGNORE_ROOT_ARG => Ok(true),
        NO_GITIGNORE_ARG => Ok(false),
        other => Err(format!(
            "expected '{GITIGNORE_ARG}' or '{NO_GITIGNORE_ARG}', got {other:?}"
        )),
    }
}

/// The shared traversal: every file under `start` that `keep` takes, visited
/// with its `root`-relative path and its directory, siblings in name order,
/// so the whole walk comes out in path order without a sort at the end.
/// Directories are read on spare cores ahead of the visiting. Prunes any
/// directory the skip rules ignore wholesale, so an ignored tree is never read
/// at all. With a glob, a dot-named entry it does not spell is skipped; without one all are
/// admitted. With `gitignore` set, entries a `.gitignore` in force ignores
/// are pruned/skipped too, starting from the walker's frames: the ones in
/// force above `start` when a repo encloses it. Inside a directory holding
/// `.git`, its `.gitignore` files apply. Symlinks are followed only as the
/// glob allows, and not at all without one; a broken link or an unreadable
/// directory contributes nothing.
fn walk(
    root: &Path,
    start: &Path,
    walker: Walk<'_>,
    keep: &(dyn Fn(&Path) -> bool + Sync),
    visit: &mut dyn FnMut(PathBuf, &Path),
) {
    let rel = start.strip_prefix(root).unwrap_or(start);
    let states = walker.glob.map_or_else(Vec::new, |glob| {
        rel.components().fold(glob.start(), |states, component| {
            glob.step(&states, component.as_os_str(), Kind::Dir)
        })
    });
    let place = Place {
        walk: walker,
        dir: start.to_path_buf(),
        rel: rel.to_path_buf(),
        states,
        linked: false,
    };
    walk_in_order(
        Box::new(place),
        &|place: Box<Place<'_>>| (*place).explore(keep),
        &mut |(rel, dir)| {
            visit(rel, &dir);
        },
    );
}

/// A directory the walk enters, and how it got there: the glob `states`
/// it is entered with, and `linked` as whether its path crosses a followed
/// symlink.
struct Place<'a> {
    walk: Walk<'a>,
    dir: PathBuf,
    rel: PathBuf,
    states: Vec<usize>,
    linked: bool,
}

type Found = (PathBuf, Arc<Path>);

impl<'a> Place<'a> {
    /// The directory's entries in walk order: each file `keep` takes, and
    /// each directory to enter.
    fn explore(self, keep: &(dyn Fn(&Path) -> bool + Sync)) -> Vec<Step<Box<Place<'a>>, Found>> {
        let Place {
            mut walk,
            dir,
            rel,
            states,
            linked,
        } = self;
        if walk.gitignore && holds_git(&dir) {
            walk.in_repo = true;
            walk.frames.clear();
        }
        if walk.in_repo
            && let Some(matcher) = load_gitignore(&dir)
        {
            walk.frames.push(Arc::new(matcher));
        }
        let taken = read_judged(
            &dir,
            &|name, seen| {
                walk.take(&dir, name, seen, &rel, &states, linked)
                    .filter(|taken| match taken {
                        Taken::File(child) => keep(child),
                        Taken::Dir { .. } => true,
                    })
            },
            taken_name,
        );
        let dir: Arc<Path> = Arc::from(dir);
        let mut steps: Vec<Step<Box<Place<'a>>, Found>> = if walk.dirs {
            taken
                .iter()
                .filter_map(|taken| match taken {
                    Taken::Dir { child, .. } => {
                        Some(Step::Leaf((child.clone(), Arc::clone(&dir))))
                    }
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        let relative = walk.glob.map_or_else(Vec::new, |glob| {
            glob.relative_entries(&states)
                .into_iter()
                .map(|(name, next)| {
                    Step::Dir(Box::new(Place {
                        walk: walk.clone(),
                        dir: dir.join(name),
                        rel: rel.join(name),
                        states: next,
                        linked,
                    }))
                })
                .collect::<Vec<_>>()
        });
        steps.extend(relative);
        let entered = taken.into_iter().map(|taken| match taken {
            Taken::Dir {
                child,
                next,
                linked,
            } => Step::Dir(Box::new(Place {
                walk: walk.clone(),
                dir: dir.join(child.file_name().unwrap_or_default()),
                rel: child,
                states: next,
                linked,
            })),
            Taken::File(child) => Step::Leaf((child, Arc::clone(&dir))),
        });
        if steps.is_empty() {
            return entered.collect();
        }
        steps.extend(entered);
        steps
    }
}

#[derive(Clone)]
struct Walk<'a> {
    ignore: &'a TableMatcher,
    glob: Option<&'a PathGlob>,
    gitignore: bool,
    /// Whether a repo encloses the walk's current position, which is what
    /// puts a `.gitignore` in force.
    in_repo: bool,
    /// The `.gitignore` files in force at the walk's current position, root
    /// first.
    frames: Vec<Arc<Gitignore>>,
    /// Whether each directory entered is also visited, as a leaf.
    dirs: bool,
}

impl Walk<'_> {
    /// What the walk makes of entry `name` of `dir`, seen as `seen`, the
    /// directory at `rel`, reached with glob `states` and `linked` as whether
    /// its path already crosses a followed symlink.
    fn take(
        &self,
        dir: &Path,
        name: &OsStr,
        seen: Seen,
        rel: &Path,
        states: &[usize],
        linked: bool,
    ) -> Option<Taken> {
        let kind = kind_of(seen, self.glob.is_some(), &|follow| {
            stat_of(&dir.join(name), follow)
        })?;
        let linked = linked || kind == Kind::LinkedDir;
        let next = if linked || kind != Kind::File {
            self.next_states(states, name, kind)
        } else {
            Vec::new()
        };
        if !self.follows(linked, &next, kind) {
            return None;
        }
        let is_dir = kind != Kind::File;
        let child = rel.join(name);
        // Only a `.gitignore` reads the full path, and building one per entry
        // is a large share of a big directory's walk.
        let path = if self.frames.is_empty() {
            PathBuf::new()
        } else {
            dir.join(name)
        };
        if !self.admits(is_dir, name, &path, &child) {
            return None;
        }
        Some(if is_dir {
            Taken::Dir {
                child,
                next,
                linked,
            }
        } else {
            Taken::File(child)
        })
    }

    /// Whether the walk takes an entry: the skip rules first, then the dot-name
    /// rule, then the `.gitignore` files in force, which never hide a path the
    /// glob spells.
    fn admits(&self, is_dir: bool, name: &OsStr, path: &Path, rel: &Path) -> bool {
        should_descend(is_dir, rel, self.ignore)
            && self.admits_name(name)
            && (self.frames.is_empty()
                || self.glob.is_some_and(|glob| glob.names(rel))
                || !is_gitignored(&self.frames, path, is_dir))
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

/// An entry the walk takes: a directory to enter, with the glob states and
/// link crossing it is entered with, or a file to visit.
#[derive(Debug, PartialEq)]
enum Taken {
    Dir {
        child: PathBuf,
        next: Vec<usize>,
        linked: bool,
    },
    File(PathBuf),
}

/// The name a taken entry sorts under.
fn taken_name(taken: &Taken) -> &OsStr {
    match taken {
        Taken::File(child) | Taken::Dir { child, .. } => child.file_name().unwrap_or_default(),
    }
}

/// What the walk makes of an entry seen as `seen`, following a symlink only
/// when `follow_links` is set; `stat` reports the entry itself, or with `true`
/// its link's target. `None` for anything else, a broken link included.
fn kind_of(seen: Seen, follow_links: bool, stat: &dyn Fn(bool) -> Option<Stat>) -> Option<Kind> {
    match seen {
        Seen::Dir => Some(Kind::Dir),
        Seen::File => Some(Kind::File),
        Seen::Other => None,
        Seen::Unknown => {
            let entry = stat(false)?;
            if entry.is_link {
                kind_of(Seen::Link, follow_links, stat)
            } else {
                classify(entry.is_dir, entry.is_file, false)
            }
        }
        Seen::Link if follow_links => {
            let target = stat(true)?;
            classify(target.is_dir, target.is_file, true)
        }
        Seen::Link => None,
    }
}

/// What a stat of a path reports.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stat {
    is_link: bool,
    is_dir: bool,
    is_file: bool,
}

/// The stat of `path`, through a symlink when `follow` is set.
fn stat_of(path: &Path, follow: bool) -> Option<Stat> {
    let metadata = if follow {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    };
    let file_type = metadata.ok()?.file_type();
    Some(Stat {
        is_link: file_type.is_symlink(),
        is_dir: file_type.is_dir(),
        is_file: file_type.is_file(),
    })
}

fn classify(is_dir: bool, is_file: bool, linked: bool) -> Option<Kind> {
    match (is_dir, is_file) {
        (true, _) if linked => Some(Kind::LinkedDir),
        (true, _) => Some(Kind::Dir),
        (_, true) => Some(Kind::File),
        _ => None,
    }
}

/// The `.gitignore` files above `start`, up to and including the repo root
/// `top`, outermost first, each read by `load`.
fn gitignores_above(
    start: &Path,
    top: &Path,
    load: &dyn Fn(&Path) -> Option<Gitignore>,
) -> Vec<Arc<Gitignore>> {
    dirs_above(start, top)
        .into_iter()
        .filter_map(load)
        .map(Arc::new)
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

/// Whether the `.gitignore` files of the repo enclosing `path` ignore it, or
/// a directory above it, as the walk would have: for a path a watcher reports
/// without having walked to it.
pub(crate) fn is_gitignored_path(path: &Path, is_dir: bool) -> bool {
    let Some(top) = path
        .parent()
        .and_then(|dir| enclosing_repo(dir, &holds_git))
    else {
        return false;
    };
    for frame in gitignores_above(path, top, &load_gitignore).iter().rev() {
        match frame.matched_path_or_any_parents(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Whether the `.gitignore` files in force mark `path` ignored. Deeper files
/// take precedence (git's rule), and a whitelisting `!pattern` un-ignores.
fn is_gitignored(frames: &[Arc<Gitignore>], path: &Path, is_dir: bool) -> bool {
    for frame in frames.iter().rev() {
        match frame.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

/// Whether the walk keeps `rel_path`. False prunes any directory whose whole
/// subtree the skip rules ignore.
fn should_descend(is_dir: bool, rel_path: &Path, ignore: &TableMatcher) -> bool {
    !is_dir || !ignore.is_ignored_dir(rel_path)
}

/// Whether a walk from the root enters the directory at `rel_path`: every
/// directory on the way, itself included, survives [`should_descend`].
pub(crate) fn reaches_dir(rel_path: &Path, ignore: &TableMatcher) -> bool {
    let mut prefix = PathBuf::new();
    rel_path.components().all(|component| {
        prefix.push(component);
        should_descend(true, &prefix, ignore)
    })
}

/// Whether `rel_path` matches `glob`.
fn is_glob_match(glob: &PathGlob, rel_path: &Path) -> bool {
    glob.is_match(rel_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn scan_directory_reporting_walks_a_known_source_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let matcher = TableMatcher::new(&[("scanner.rs", "source")], &[]).unwrap();
        let mut counts = Vec::new();

        let rows = scan_directory_reporting(&root, &matcher, &mut |count| counts.push(count));

        assert_eq!(rows, vec![(root.join("scanner.rs"), "source".to_owned())]);
        assert_eq!(counts.first(), Some(&1));
        assert!(counts.windows(2).all(|pair| pair[1] == pair[0] + 1));
    }

    // Real directory-walk behavior is covered by `tests/scanner.rs`
    // (unit-lint isolation); only the pure predicate is tested here.

    const LINK: Stat = Stat {
        is_link: true,
        is_dir: false,
        is_file: false,
    };
    const DIR: Stat = Stat {
        is_link: false,
        is_dir: true,
        is_file: false,
    };
    const FILE: Stat = Stat {
        is_link: false,
        is_dir: false,
        is_file: true,
    };

    /// A stat of a symlink to `target`.
    fn link_to(target: Stat) -> impl Fn(bool) -> Option<Stat> {
        move |follow| Some(if follow { target } else { LINK })
    }

    fn unstatted(_follow: bool) -> Option<Stat> {
        panic!("an entry its listing named needs no stat")
    }

    #[test]
    fn kind_of_takes_a_directory_and_a_file_as_they_were_seen() {
        assert_eq!(kind_of(Seen::Dir, true, &unstatted), Some(Kind::Dir));
        assert_eq!(kind_of(Seen::File, true, &unstatted), Some(Kind::File));
    }

    #[test]
    fn kind_of_drops_a_device_and_an_unfollowed_link() {
        assert_eq!(kind_of(Seen::Other, true, &unstatted), None);
        assert_eq!(kind_of(Seen::Link, false, &link_to(DIR)), None);
    }

    #[test]
    fn kind_of_follows_a_link_to_its_target_when_asked() {
        assert_eq!(
            kind_of(Seen::Link, true, &link_to(DIR)),
            Some(Kind::LinkedDir)
        );
        assert_eq!(kind_of(Seen::Link, true, &link_to(FILE)), Some(Kind::File));
        assert_eq!(kind_of(Seen::Link, true, &|_| None), None);
    }

    #[test]
    fn kind_of_stats_an_entry_its_listing_did_not_type() {
        assert_eq!(
            kind_of(Seen::Unknown, false, &|_| Some(DIR)),
            Some(Kind::Dir)
        );
        assert_eq!(
            kind_of(Seen::Unknown, false, &|_| Some(FILE)),
            Some(Kind::File)
        );
        assert_eq!(kind_of(Seen::Unknown, false, &link_to(DIR)), None);
        assert_eq!(
            kind_of(Seen::Unknown, true, &link_to(DIR)),
            Some(Kind::LinkedDir)
        );
        assert_eq!(kind_of(Seen::Unknown, true, &|_| None), None);
    }

    #[test]
    fn stat_of_reports_a_directory_a_file_and_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "").unwrap();
        assert_eq!(stat_of(dir.path(), false), Some(DIR));
        assert_eq!(stat_of(&file, true), Some(FILE));
        assert_eq!(stat_of(&dir.path().join("missing"), true), None);
    }

    fn take(
        walk: &Walk<'_>,
        name: &str,
        seen: Seen,
        states: &[usize],
        linked: bool,
    ) -> Option<Taken> {
        walk.take(
            Path::new("/r"),
            OsStr::new(name),
            seen,
            Path::new(""),
            states,
            linked,
        )
    }

    #[test]
    fn take_enters_a_real_directory_a_component_can_enter_off_any_link() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*/x").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
        assert_eq!(
            take(&walk, "a", Seen::Dir, &[0], false),
            Some(Taken::Dir {
                child: PathBuf::from("a"),
                next: vec![1],
                linked: false,
            })
        );
    }

    #[test]
    fn take_keeps_a_file_off_a_link_for_the_whole_path_to_judge() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*/x").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
        assert_eq!(
            take(&walk, "y", Seen::File, &[1], false),
            Some(Taken::File(PathBuf::from("y")))
        );
    }

    #[test]
    fn take_drops_a_file_below_a_link_the_glob_does_not_reach() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*/x").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
        assert_eq!(take(&walk, "y", Seen::File, &[1], true), None);
        assert_eq!(
            take(&walk, "x", Seen::File, &[1], true),
            Some(Taken::File(PathBuf::from("x")))
        );
    }

    fn explored_dirs(place: Place<'_>) -> Vec<Place<'_>> {
        place
            .explore(&|_| true)
            .into_iter()
            .filter_map(|step| match step {
                Step::Dir(child) => Some(*child),
                Step::Leaf(_) => None,
            })
            .collect()
    }

    #[test]
    fn scan_dirs_lists_the_start_and_every_directory_the_scan_enters() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a").join("b")).unwrap();
        fs::create_dir_all(root.path().join("a").join("node_modules").join("pkg")).unwrap();
        fs::create_dir_all(root.path().join("a").join(".dirsql")).unwrap();
        fs::write(root.path().join("a").join("f.md"), "").unwrap();
        let ignore = TableMatcher::new(&[(".dirsql/*", "t")], &["**/node_modules/**"]).unwrap();
        let a = root.path().join("a");
        let mut dirs = scan_dirs(root.path(), &a, &ignore);
        dirs.sort();
        assert_eq!(dirs, vec![a.clone(), a.join(".dirsql"), a.join("b")]);
    }

    #[test]
    fn scan_dirs_from_the_root_is_just_the_root_when_it_holds_no_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("f.md"), "").unwrap();
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        assert_eq!(
            scan_dirs(root.path(), root.path(), &ignore),
            vec![root.path().to_path_buf()]
        );
    }

    #[test]
    fn explore_enters_a_dirsql_directory_below_the_top_level() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("a").join(".dirsql")).unwrap();
        let ignore = TableMatcher::new(&[("**/.dirsql/*", "t")], &[]).unwrap();
        let top = Place {
            walk: walk_with(&ignore, None, Vec::new()),
            dir: root.path().to_path_buf(),
            rel: PathBuf::new(),
            states: Vec::new(),
            linked: false,
        };
        let [a] = <[Place<'_>; 1]>::try_from(explored_dirs(top)).ok().unwrap();
        let nested: Vec<PathBuf> = explored_dirs(a).into_iter().map(|p| p.rel).collect();
        assert_eq!(nested, vec![Path::new("a").join(".dirsql")]);
    }

    #[test]
    fn explore_enters_a_current_directory_component_the_glob_spells() {
        let root = tempfile::tempdir().unwrap();
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("./x").unwrap();
        let top = Place {
            walk: walk_with(&ignore, Some(&glob), Vec::new()),
            dir: root.path().to_path_buf(),
            rel: PathBuf::new(),
            states: glob.start(),
            linked: false,
        };
        let [dot] = <[Place<'_>; 1]>::try_from(explored_dirs(top)).ok().unwrap();
        assert_eq!(dot.rel, Path::new("."));
        assert_eq!(dot.dir, root.path().join("."));
    }

    #[test]
    fn relative_entries_name_only_the_components_the_glob_spells() {
        let glob = compile_glob("*/../x").unwrap();
        let states = glob.step(&glob.start(), OsStr::new("docs"), Kind::Dir);
        let entries = glob.relative_entries(&states);
        assert_eq!(
            entries.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            vec![".."]
        );
        let plain = compile_glob("*/x").unwrap();
        let states = plain.step(&plain.start(), OsStr::new("docs"), Kind::Dir);
        assert!(plain.relative_entries(&states).is_empty());
    }

    #[test]
    fn a_glob_spells_relative_components_only_when_a_word_has_one() {
        assert!(compile_glob("a/./b").unwrap().spells_relative);
        assert!(compile_glob("a/../b").unwrap().spells_relative);
        assert!(!compile_glob("a/b").unwrap().spells_relative);
    }

    #[test]
    fn reaches_dir_enters_the_root_and_unignored_directories() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(reaches_dir(Path::new(""), &ignore));
        assert!(reaches_dir(Path::new("src/deep"), &ignore));
        assert!(reaches_dir(Path::new("a/.dirsql"), &ignore));
    }

    #[test]
    fn reaches_dir_stops_at_an_ignored_directory_and_below_it() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(!reaches_dir(Path::new("node_modules"), &ignore));
        assert!(!reaches_dir(Path::new("apps/node_modules/pkg"), &ignore));
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
        assert!(compile_glob("[z-a]").is_err());
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
            true,
            Path::new("apps/node_modules"),
            &ignore
        ));
    }

    #[test]
    fn should_descend_keeps_an_ordinary_directory() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(true, Path::new("docs"), &ignore));
    }

    #[test]
    fn should_descend_keeps_a_file_even_when_a_subtree_pattern_names_it() {
        let ignore = TableMatcher::new(&[], &["**/node_modules/**"]).unwrap();
        assert!(should_descend(
            false,
            Path::new("apps/node_modules"),
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
    fn frame(dir: &str, lines: &[&str]) -> Arc<Gitignore> {
        let mut builder = GitignoreBuilder::new(Path::new(dir));
        for line in lines {
            builder.add_line(None, line).unwrap();
        }
        Arc::new(builder.build().unwrap())
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
    fn gitignores_above_loads_the_repo_root_ignore_file() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = package.ancestors().nth(2).unwrap();
        let start = package.join("src");

        assert!(!gitignores_above(&start, root, &load_gitignore).is_empty());
    }

    #[test]
    fn gitignores_above_loads_the_ancestors_holding_one_outermost_first() {
        let load = |dir: &Path| {
            (dir != Path::new("/r/a")).then(|| {
                let mut builder = GitignoreBuilder::new(dir);
                builder.add_line(None, "*.log").unwrap();
                builder.build().unwrap()
            })
        };
        let frames = gitignores_above(Path::new("/r/a/b/c"), Path::new("/r"), &load);
        let roots: Vec<&Path> = frames.iter().map(|frame| frame.path()).collect();
        assert_eq!(roots, vec![Path::new("/r"), Path::new("/r/a/b")]);
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
        frames: Vec<Arc<Gitignore>>,
    ) -> Walk<'a> {
        Walk {
            ignore,
            glob,
            gitignore: !frames.is_empty(),
            in_repo: !frames.is_empty(),
            frames,
            dirs: false,
        }
    }

    #[test]
    fn admits_an_ordinary_file_under_no_gitignore() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("**").unwrap();
        let walk = walk_with(&ignore, Some(&glob), Vec::new());
        assert!(walk.admits(
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
            false,
            OsStr::new(".env"),
            Path::new("/r/.env"),
            Path::new(".env")
        ));
        assert!(!walk.admits(
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
            false,
            OsStr::new(".env"),
            Path::new("/r/sub/.env"),
            Path::new("sub/.env")
        ));
        assert!(!walk.admits(
            true,
            OsStr::new(".hidden"),
            Path::new("/r/.hidden"),
            Path::new(".hidden")
        ));
    }

    #[test]
    fn a_table_walk_enters_a_node_modules_directory() {
        let ignore = TableMatcher::new(&[("**/*.js", "t")], &[]).unwrap();
        let walk = walk_with(&ignore, Some(ignore.walk_glob()), Vec::new());
        let admits = |is_dir, name: &str| {
            walk.admits(is_dir, OsStr::new(name), Path::new(name), Path::new(name))
        };
        assert!(admits(true, "node_modules"));
        assert!(admits(false, "node_modules"));
    }

    #[test]
    fn a_match_with_an_unspelled_dot_component_is_hidden() {
        let glob = compile_glob("**/*.md").unwrap();
        assert!(glob.is_match_unhidden(Path::new("sub/b.md")));
        assert!(!glob.is_match_unhidden(Path::new(".env.md")));
        assert!(!glob.is_match_unhidden(Path::new(".hid/z.md")));
        let spelled = compile_glob(".hid/*.md").unwrap();
        assert!(spelled.is_match_unhidden(Path::new(".hid/z.md")));
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
            false,
            OsStr::new("debug.log"),
            Path::new("/r/debug.log"),
            Path::new("debug.log")
        ));
        assert!(walk.admits(
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
    fn a_double_star_never_consumes_a_dot_named_entry() {
        let glob = compile_glob("**/.*").unwrap();
        assert_eq!(states_after(&glob, &[(".cache", Kind::Dir)]), vec![2]);
        assert!(states_after(&glob, &[(".cache", Kind::Dir), (".z", Kind::File)]).is_empty());
    }

    #[test]
    fn only_a_component_spelled_with_a_dot_consumes_a_dot_named_entry() {
        let glob = compile_glob("*/x").unwrap();
        assert!(states_after(&glob, &[(".d", Kind::Dir)]).is_empty());
        let glob = compile_glob(".*/x").unwrap();
        assert_eq!(states_after(&glob, &[(".d", Kind::Dir)]), vec![1]);
    }

    #[test]
    fn a_spelled_dot_component_consumes_only_the_names_it_matches() {
        let glob = compile_glob(".claude/x").unwrap();
        assert_eq!(states_after(&glob, &[(".claude", Kind::Dir)]), vec![1]);
        assert!(states_after(&glob, &[(".env", Kind::Dir)]).is_empty());
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
    fn a_glob_without_words_follows_no_link() {
        let glob = compile_globs(&[]).unwrap();
        assert!(glob.components.is_empty());
        assert!(!glob.reaches(&glob.start(), Kind::LinkedDir));
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
            dirs: false,
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
            dirs: false,
        };
        assert!(walk.follows(false, &[1], Kind::Dir));
        assert!(!walk.follows(false, &[2], Kind::Dir));
        assert!(!walk.follows(false, &[], Kind::Dir));
    }

    #[test]
    fn enters_every_real_directory_when_the_glob_has_no_components() {
        let ignore = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_globs(&[]).unwrap();
        let walk = Walk {
            ignore: &ignore,
            glob: Some(&glob),
            gitignore: false,
            in_repo: false,
            frames: Vec::new(),
            dirs: false,
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

    #[test]
    fn a_gitignored_file_the_glob_names_outright_is_listed_and_a_wildcard_still_hides_it() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        fs::write(repo.path().join(".gitignore"), "*.log\ndist/\n").unwrap();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(repo.path().join("dist/out.log"), "").unwrap();
        fs::write(repo.path().join("a.log"), "").unwrap();
        let none = TableMatcher::new(&[], &[]).unwrap();
        let scan =
            |pattern: &str| scan_glob(repo.path(), &compile_glob(pattern).unwrap(), &none, true);

        assert_eq!(scan("a.log"), vec![PathBuf::from("a.log")]);
        assert_eq!(scan("dist/out.log"), vec![PathBuf::from("dist/out.log")]);
        assert_eq!(scan("{a,b}.log"), vec![PathBuf::from("a.log")]);
        assert!(scan("*.log").is_empty());
    }

    #[test]
    fn a_named_directory_lists_though_gitignored_and_a_wildcard_directory_does_not() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        fs::write(repo.path().join(".gitignore"), "dist/\n").unwrap();
        fs::create_dir(repo.path().join("dist")).unwrap();
        fs::write(repo.path().join("dist/a.js"), "").unwrap();
        let scan = |named: &[&str]| {
            let matcher = TableMatcher::new(&[("dist/*", "t")], &[])
                .unwrap()
                .with_gitignore(true)
                .with_named_dirs(&named.iter().map(|d| d.to_string()).collect::<Vec<_>>());
            scan_directory(repo.path(), &matcher)
        };

        assert_eq!(
            scan(&["dist"]),
            vec![(repo.path().join("dist/a.js"), "t".to_string())]
        );
        assert!(scan(&[]).is_empty());
    }

    #[test]
    fn a_checked_root_that_gitignore_ignores_yields_nothing_unless_it_is_unchecked() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        fs::write(repo.path().join(".gitignore"), "dist/\n").unwrap();
        fs::create_dir_all(repo.path().join("dist/sub")).unwrap();
        fs::write(repo.path().join("dist/sub/o.js"), "").unwrap();
        let none = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*").unwrap();
        let scan = |dir: &str, check_root: bool| {
            scan_glob_checking_root(&repo.path().join(dir), &glob, &none, true, check_root)
        };

        assert!(scan("dist", true).is_empty());
        assert!(scan("dist/sub", true).is_empty());
        assert_eq!(scan("dist/sub", false), vec![PathBuf::from("o.js")]);
        assert_eq!(scan("", true), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_checked_root_that_gitignore_does_not_ignore_is_scanned() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        fs::write(repo.path().join(".gitignore"), "dist/\n").unwrap();
        fs::create_dir(repo.path().join("src")).unwrap();
        fs::write(repo.path().join("src/a.js"), "").unwrap();
        let none = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*.js").unwrap();

        assert_eq!(
            scan_glob_checking_root(&repo.path().join("src"), &glob, &none, true, true),
            vec![PathBuf::from("a.js")]
        );
    }

    #[test]
    fn a_checked_root_holding_its_own_git_dir_is_not_judged_by_the_repo_above() {
        let outer = tempfile::tempdir().unwrap();
        fs::create_dir(outer.path().join(".git")).unwrap();
        fs::write(outer.path().join(".gitignore"), "inner/\n").unwrap();
        fs::create_dir_all(outer.path().join("inner/.git")).unwrap();
        fs::write(outer.path().join("inner/a.js"), "").unwrap();
        let none = TableMatcher::new(&[], &[]).unwrap();
        let glob = compile_glob("*.js").unwrap();

        assert_eq!(
            scan_glob_checking_root(&outer.path().join("inner"), &glob, &none, true, true),
            vec![PathBuf::from("a.js")]
        );
    }

    #[test]
    fn the_gitignore_root_switch_turns_gitignore_on() {
        assert_eq!(parse_gitignore_arg(GITIGNORE_ROOT_ARG), Ok(true));
    }

    #[test]
    fn is_gitignored_path_ignores_a_path_the_repo_gitignore_matches() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        fs::write(repo.path().join(".gitignore"), "*.log\n").unwrap();

        assert!(is_gitignored_path(&repo.path().join("debug.log"), false));
        assert!(!is_gitignored_path(&repo.path().join("app.js"), false));
    }
}
