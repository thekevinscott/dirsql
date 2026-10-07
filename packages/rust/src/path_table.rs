//! Turning the string a user writes where a table name goes into a concrete
//! scan: a directory to walk, a glob relative to it, and the base the `path`
//! column is reported against.
//!
//! Every decision here is a pure function of the written string plus one
//! filesystem question (*is this a directory?*), which is injected so the
//! rules can be tested without a filesystem. The path syntax is a type
//! parameter, so the Windows rules are testable on any host.

use std::path::{Path, PathBuf};

use typed_path::{
    Utf8Component, Utf8Encoding, Utf8NativeEncoding, Utf8Path, Utf8PathBuf, Utf8UnixEncoding,
    Utf8WindowsComponent, Utf8WindowsEncoding, Utf8WindowsPath,
};

/// Directories a path-table scan skips, at any depth. They are skipped only
/// *beneath* the literal part of the path you write, so naming one explicitly
/// still scans it. See [`PathTable::default_ignores`] for `node_modules`
/// named after a glob.
pub const DEFAULT_IGNORES: [&str; 2] = [NODE_MODULES_IGNORE, "**/.git/**"];

const NODE_MODULES_IGNORE: &str = "**/node_modules/**";

/// The glob a directory expands to: one level, like `ls`. Any depth is
/// spelled explicitly as `**`.
const DIRECTORY_GLOB: &str = "*";

/// A resolved path-table: what to walk, what to match, and what to report.
#[derive(Debug, PartialEq)]
pub struct PathTable {
    /// Directory the scan walks.
    pub root: PathBuf,
    /// Glob matched against paths relative to [`root`](Self::root).
    pub glob: String,
    /// Prepended to each matched relative path before the stat columns are
    /// computed: the directories a `./` table named ahead of its glob, so its
    /// paths read as index-root-relative; the absolute scan root for the rest.
    pub path_prefix: String,
}

impl PathTable {
    /// The [`DEFAULT_IGNORES`] this table applies. A glob with a
    /// `node_modules` component names that directory wherever the component
    /// sits, so its skip rule is dropped.
    pub fn default_ignores(&self) -> impl Iterator<Item = &'static str> {
        let names_node_modules = self.glob.split('/').any(|c| c == "node_modules");
        DEFAULT_IGNORES
            .into_iter()
            .filter(move |rule| !(names_node_modules && *rule == NODE_MODULES_IGNORE))
    }
}

/// What a name SQLite could not find turns out to be.
#[derive(Debug, PartialEq)]
pub enum Resolution {
    /// A path-table; scan this.
    Table(PathTable),
    /// A bare glob: almost certainly a path-table missing its `./`.
    Hint,
    /// A `~/` path on a system with no home directory.
    NoHome,
    /// An ordinary identifier. Not ours; the SQLite error stands.
    NotAPath,
}

/// The per-platform rules for which written names are absolute and how a
/// resolved scan root is reported.
trait Syntax: Utf8Encoding + Sized {
    const HOME_PREFIXES: &'static [&'static str];
    const SEPARATORS: &'static [char];

    fn is_rooted(name: &str) -> bool;

    fn reported(path: &Utf8Path<Self>) -> String;
}

impl Syntax for Utf8UnixEncoding {
    const HOME_PREFIXES: &'static [&'static str] = &["~/"];
    const SEPARATORS: &'static [char] = &['/'];

    fn is_rooted(name: &str) -> bool {
        name.starts_with('/')
    }

    fn reported(path: &Utf8Path<Self>) -> String {
        path.as_str().to_string()
    }
}

impl Syntax for Utf8WindowsEncoding {
    const HOME_PREFIXES: &'static [&'static str] = &["~/", "~\\"];
    const SEPARATORS: &'static [char] = &['/', '\\'];

    fn is_rooted(name: &str) -> bool {
        Utf8WindowsPath::new(name).has_root()
    }

    /// Reported with `/`, matching the index-root-relative paths. A verbatim
    /// (`\\?\`) path is the exception: there `/` is not a separator.
    fn reported(path: &Utf8Path<Self>) -> String {
        let verbatim = matches!(
            path.components().next(),
            Some(Utf8WindowsComponent::Prefix(prefix)) if prefix.kind().is_verbatim()
        );
        if verbatim {
            path.as_str().to_string()
        } else {
            path.as_str().replace('\\', "/")
        }
    }
}

/// Whether `name` contains a character that makes it a glob rather than a
/// literal path.
fn has_glob_metacharacter(name: &str) -> bool {
    name.contains(['*', '?', '[', '{'])
}

/// Resolve `name` against the index root, reporting what kind of thing it is.
///
/// `is_dir` answers the one filesystem question the rules need; production
/// passes `Path::is_dir`.
pub fn resolve(
    name: &str,
    index_root: &Path,
    home: Option<&Path>,
    is_dir: &dyn Fn(&Path) -> bool,
) -> Resolution {
    resolve_as::<Utf8NativeEncoding>(name, index_root, home, is_dir)
}

fn resolve_as<S: Syntax>(
    name: &str,
    index_root: &Path,
    home: Option<&Path>,
    is_dir: &dyn Fn(&Path) -> bool,
) -> Resolution {
    let target = trailing_separator_as_star::<S>(name);
    if let Some(rest) = target.strip_prefix("./") {
        return Resolution::Table(split_relative(index_root, rest, is_dir));
    }

    match absolute_target::<S>(&target, index_root, home) {
        Some(Some(target)) => Resolution::Table(split_absolute(&target, is_dir)),
        Some(None) => Resolution::NoHome,
        None if has_glob_metacharacter(name) => Resolution::Hint,
        None => Resolution::NotAPath,
    }
}

/// `glob` as a path-table reads the same string after `FROM './`: a trailing
/// separator or a wholly literal name that is a directory under `index_root`
/// lists the files directly inside it.
pub fn directory_as_glob(glob: &str, index_root: &Path, is_dir: &dyn Fn(&Path) -> bool) -> String {
    let target = trailing_separator_as_star::<Utf8UnixEncoding>(glob);
    if !target.is_empty() && !has_glob_metacharacter(&target) && is_dir(&index_root.join(&target)) {
        return format!("{target}/{DIRECTORY_GLOB}");
    }
    target
}

/// Where a config `[[table]] glob` anchors, and the glob to match beneath it.
///
/// A glob with a path-table prefix (`/`, `../`, `~/`, a drive or UNC root)
/// anchors at its literal directory chain; any other glob, `./` included,
/// anchors at the config's own directory. `None` means a `~/` glob with no
/// home directory to resolve it against.
pub(crate) fn config_anchor(
    glob: &str,
    config_dir: &Path,
    home: Option<&Path>,
) -> Option<(PathBuf, String)> {
    config_anchor_as::<Utf8NativeEncoding>(glob, config_dir, home)
}

fn config_anchor_as<S: Syntax>(
    glob: &str,
    config_dir: &Path,
    home: Option<&Path>,
) -> Option<(PathBuf, String)> {
    if let Some(rest) = glob.strip_prefix("./") {
        return Some((config_dir.to_path_buf(), rest.to_string()));
    }
    let Some(target) = absolute_target::<S>(glob, config_dir, home) else {
        return Some((config_dir.to_path_buf(), glob.to_string()));
    };
    let target = target?;
    let (literal, rest) = split_at_first_glob(&target);
    if !rest.is_empty() {
        return Some((PathBuf::from(literal.as_str()), rest));
    }
    let name = literal.file_name()?.to_string();
    let parent = literal.parent().unwrap_or(&literal);
    Some((PathBuf::from(parent.as_str()), name))
}

/// A trailing separator is `*` appended, so `./*/` is `./*/*` (like `ls */`)
/// rather than a `./*` whose slash the path parser would drop.
fn trailing_separator_as_star<S: Syntax>(name: &str) -> String {
    if name.ends_with(S::SEPARATORS) {
        format!("{name}*")
    } else {
        name.to_string()
    }
}

/// Split a `./`-relative target into the directory to walk beneath the index
/// root and the glob to match there. The walk starts at the literal prefix
/// and `path` is reported under it, so the rows read as if the index root
/// had been walked whole. The prefix is kept as written, `//` and `.` and
/// `..` included, as the shell keeps it; past the first wildcard, repeated
/// separators collapse.
fn split_relative(index_root: &Path, rest: &str, is_dir: &dyn Fn(&Path) -> bool) -> PathTable {
    let parts: Vec<&str> = rest.split('/').collect();
    let (literal, glob) = match parts.iter().position(|p| has_glob_metacharacter(p)) {
        Some(at) => (parts[..at].join("/"), glob_of(&parts[at..])),
        None if is_dir(&index_root.join(rest)) => (rest.to_string(), DIRECTORY_GLOB.to_string()),
        None => {
            let (name, parent) = parts.split_last().expect("split yields a part");
            (parent.join("/"), (*name).to_string())
        }
    };
    let root = if literal.is_empty() {
        index_root.to_path_buf()
    } else {
        index_root.join(&literal)
    };
    // A prefix already ending in a separator is not given another when a path
    // is reported under it, so a written `//` needs the second one here.
    let path_prefix = if literal.ends_with('/') {
        format!("{literal}/")
    } else {
        literal
    };
    PathTable {
        root,
        glob,
        path_prefix,
    }
}

fn glob_of(parts: &[&str]) -> String {
    let kept: Vec<&str> = parts.iter().copied().filter(|p| !p.is_empty()).collect();
    kept.join("/")
}

fn typed<S: Syntax>(path: &Path) -> Utf8PathBuf<S> {
    Utf8PathBuf::from(path.to_string_lossy().into_owned())
}

/// The absolute path a non-`./` path-table names, with `.` and `..` folded out.
///
/// `None` means the name is not a path at all. `Some(None)` means it is a `~/`
/// path but no home directory could be found.
fn absolute_target<S: Syntax>(
    name: &str,
    index_root: &Path,
    home: Option<&Path>,
) -> Option<Option<Utf8PathBuf<S>>> {
    if let Some(rest) = S::HOME_PREFIXES.iter().find_map(|p| name.strip_prefix(p)) {
        return Some(home.map(|h| normalize(&typed::<S>(h).join(rest))));
    }
    if name.starts_with("../") {
        return Some(Some(normalize(&typed::<S>(index_root).join(name))));
    }
    if S::is_rooted(name) {
        return Some(Some(normalize(Utf8Path::new(name))));
    }
    None
}

/// Fold `.` and `..` out of `path` lexically. Purely textual: a `..` is not
/// resolved through a symlink, which keeps the answer a function of the string
/// the user wrote.
fn normalize<S: Syntax>(path: &Utf8Path<S>) -> Utf8PathBuf<S> {
    let mut kept: Vec<&str> = Vec::new();
    let mut normal = 0;
    let mut rooted = false;
    for component in path.components() {
        if component.is_current() {
            continue;
        }
        if component.is_parent() {
            // A `..` that cannot pop is kept on a relative path (it still
            // means something) but dropped at the filesystem root, which has
            // no parent to climb to.
            if normal > 0 {
                kept.pop();
                normal -= 1;
            } else if !rooted {
                kept.push(component.as_str());
            }
            continue;
        }
        if component.is_normal() {
            normal += 1;
        } else if component.is_root() {
            rooted = true;
        }
        kept.push(component.as_str());
    }

    let mut out = Utf8PathBuf::new();
    for component in kept {
        out.push(component);
    }
    out
}

/// Split an absolute path-table target into the directory to walk and the glob
/// to match beneath it.
fn split_absolute<S: Syntax>(target: &Utf8Path<S>, is_dir: &dyn Fn(&Path) -> bool) -> PathTable {
    let (literal, glob) = split_target(target, is_dir);
    table_at(&literal, glob)
}

/// The directory to walk and the glob to match beneath it. A wholly literal
/// target is a directory (list it one level deep) or a single file (match
/// exactly that name beneath its parent).
fn split_target<S: Syntax>(
    target: &Utf8Path<S>,
    is_dir: &dyn Fn(&Path) -> bool,
) -> (Utf8PathBuf<S>, String) {
    let (literal, rest) = split_at_first_glob(target);

    if !rest.is_empty() {
        return (literal, rest);
    }
    if is_dir(Path::new(literal.as_str())) {
        return (literal, DIRECTORY_GLOB.to_string());
    }

    let name = literal
        .file_name()
        .map(str::to_string)
        .unwrap_or_else(|| DIRECTORY_GLOB.to_string());
    let parent = literal.parent().unwrap_or(&literal).to_path_buf();
    (parent, name)
}

/// A path-table rooted at `root`, reporting absolute paths.
fn table_at<S: Syntax>(root: &Utf8Path<S>, glob: String) -> PathTable {
    PathTable {
        path_prefix: S::reported(root),
        root: PathBuf::from(root.as_str()),
        glob,
    }
}

/// Split `target` at its first glob-bearing component: the literal directory
/// chain ahead of it, and the rest as a `/`-joined glob.
fn split_at_first_glob<S: Syntax>(target: &Utf8Path<S>) -> (Utf8PathBuf<S>, String) {
    let mut literal = Utf8PathBuf::new();
    let mut rest: Vec<&str> = Vec::new();

    for component in target.components() {
        let text = component.as_str();
        let is_glob = component.is_normal() && has_glob_metacharacter(text);
        if rest.is_empty() && !is_glob {
            literal.push(text);
        } else {
            rest.push(text);
        }
    }

    (literal, rest.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/index";

    fn nothing_is_a_dir(_: &Path) -> bool {
        false
    }

    fn everything_is_a_dir(_: &Path) -> bool {
        true
    }

    fn resolve_with(name: &str, is_dir: &dyn Fn(&Path) -> bool) -> Resolution {
        resolve(name, Path::new(ROOT), Some(Path::new("/home/u")), is_dir)
    }

    fn table(name: &str, is_dir: &dyn Fn(&Path) -> bool) -> PathTable {
        match resolve_with(name, is_dir) {
            Resolution::Table(t) => t,
            other => panic!("expected a path-table, got {other:?}"),
        }
    }

    fn directory_glob(glob: &str, is_dir: &dyn Fn(&Path) -> bool) -> String {
        directory_as_glob(glob, Path::new(ROOT), is_dir)
    }

    #[test]
    fn a_literal_directory_name_lists_one_level() {
        assert_eq!(directory_glob("docs", &everything_is_a_dir), "docs/*");
    }

    #[test]
    fn a_literal_directory_is_looked_up_under_the_index_root() {
        let only_docs = |p: &Path| p == Path::new("/index/docs");
        assert_eq!(directory_glob("docs", &only_docs), "docs/*");
        assert_eq!(directory_glob("other", &only_docs), "other");
    }

    #[test]
    fn a_literal_file_name_is_left_alone() {
        assert_eq!(directory_glob("top.md", &nothing_is_a_dir), "top.md");
    }

    #[test]
    fn a_glob_is_left_alone_even_when_a_directory_has_its_name() {
        assert_eq!(
            directory_glob("docs/*.md", &everything_is_a_dir),
            "docs/*.md"
        );
    }

    #[test]
    fn a_trailing_separator_lists_one_level() {
        assert_eq!(directory_glob("docs/", &nothing_is_a_dir), "docs/*");
    }

    #[test]
    fn an_empty_glob_is_left_alone() {
        assert_eq!(directory_glob("", &everything_is_a_dir), "");
    }

    #[test]
    fn default_ignores_cover_vcs_and_dependency_directories() {
        assert_eq!(DEFAULT_IGNORES, ["**/node_modules/**", "**/.git/**"]);
    }

    fn table_globbing(glob: &str) -> PathTable {
        PathTable {
            root: PathBuf::from("/r"),
            glob: glob.to_string(),
            path_prefix: String::new(),
        }
    }

    fn default_ignores_of(glob: &str) -> Vec<&'static str> {
        table_globbing(glob).default_ignores().collect()
    }

    #[test]
    fn a_glob_not_naming_node_modules_keeps_every_default_ignore() {
        assert_eq!(default_ignores_of("**"), DEFAULT_IGNORES);
        assert_eq!(default_ignores_of("**/*.js"), DEFAULT_IGNORES);
        assert_eq!(default_ignores_of("node_modules*/*"), DEFAULT_IGNORES);
    }

    #[test]
    fn a_glob_naming_node_modules_anywhere_drops_its_ignore() {
        assert_eq!(default_ignores_of("**/node_modules/**"), ["**/.git/**"]);
        assert_eq!(default_ignores_of("*/node_modules/*/*"), ["**/.git/**"]);
        assert_eq!(default_ignores_of("node_modules"), ["**/.git/**"]);
    }

    #[test]
    fn a_double_separator_in_the_literal_prefix_is_kept_as_written() {
        let t = table("./docs//a.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs/"));
        assert_eq!(t.glob, "a.md");
        assert_eq!(t.path_prefix, "docs//");
    }

    #[test]
    fn a_current_directory_component_in_the_literal_prefix_is_kept_as_written() {
        let t = table("./docs/./*.md", &nothing_is_a_dir);
        assert_eq!(t.glob, "*.md");
        assert_eq!(t.path_prefix, "docs/.");
    }

    #[test]
    fn a_current_directory_component_after_a_wildcard_stays_in_the_glob() {
        let t = table("./*/./a.md", &nothing_is_a_dir);
        assert_eq!(t.glob, "*/./a.md");
    }

    #[test]
    fn a_double_separator_after_a_wildcard_collapses() {
        let t = table("./*//a.md", &nothing_is_a_dir);
        assert_eq!(t.glob, "*/a.md");
    }

    #[test]
    fn has_glob_metacharacter_spots_each_metacharacter() {
        assert!(has_glob_metacharacter("a*"));
        assert!(has_glob_metacharacter("a?"));
        assert!(has_glob_metacharacter("a[bc]"));
        assert!(has_glob_metacharacter("{a,b}"));
    }

    #[test]
    fn has_glob_metacharacter_is_false_for_a_literal_path() {
        assert!(!has_glob_metacharacter("docs/a.md"));
    }

    #[test]
    fn a_bare_dot_slash_lists_the_index_root_one_level_deep() {
        let t = table("./", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "*");
        assert_eq!(t.path_prefix, "");
    }

    #[test]
    fn a_relative_directory_lists_one_level() {
        let t = table("./docs", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "*");
        assert_eq!(t.path_prefix, "docs");
    }

    #[test]
    fn a_relative_directory_with_a_trailing_slash_lists_one_level() {
        let t = table("./docs/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn a_trailing_slash_after_a_glob_is_a_star_appended() {
        let t = table("./*/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "*/*");
        assert_eq!(t.path_prefix, "");
    }

    #[test]
    fn a_trailing_slash_after_a_nested_glob_is_a_star_appended() {
        let t = table("./docs/*/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "*/*");
        assert_eq!(t.path_prefix, "docs");
    }

    #[test]
    fn a_trailing_slash_after_a_double_star_is_a_star_appended() {
        assert_eq!(table("./**/", &everything_is_a_dir).glob, "**/*");
    }

    #[test]
    fn an_absolute_trailing_slash_after_a_glob_is_a_star_appended() {
        let t = table("/var/*/", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var"));
        assert_eq!(t.glob, "*/*");
    }

    #[test]
    fn a_parent_relative_trailing_slash_after_a_glob_is_a_star_appended() {
        let t = table("../*/", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/"));
        assert_eq!(t.glob, "*/*");
    }

    #[test]
    fn a_home_relative_trailing_slash_after_a_glob_is_a_star_appended() {
        let t = table("~/*/", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/home/u"));
        assert_eq!(t.glob, "*/*");
    }

    #[test]
    fn an_absolute_directory_with_a_trailing_slash_lists_one_level() {
        let t = table("/var/log/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn a_plain_identifier_with_a_trailing_slash_is_not_a_path() {
        assert_eq!(
            resolve_with("users/", &nothing_is_a_dir),
            Resolution::NotAPath
        );
    }

    #[test]
    fn an_explicit_star_is_the_same_as_the_bare_dot_slash() {
        assert_eq!(
            table("./*", &everything_is_a_dir),
            table("./", &everything_is_a_dir)
        );
    }

    #[test]
    fn a_double_star_scans_every_depth() {
        let t = table("./**", &everything_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "**");
        assert_eq!(t.path_prefix, "");
    }

    #[test]
    fn a_relative_recursive_glob_is_used_as_written() {
        let t = table("./docs/**/*.md", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "**/*.md");
        assert_eq!(t.path_prefix, "docs");
    }

    #[test]
    fn a_relative_single_file_roots_at_its_parent() {
        let t = table("./docs/a.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "a.md");
        assert_eq!(t.path_prefix, "docs");
    }

    #[test]
    fn a_relative_top_level_file_roots_at_the_index_root() {
        let t = table("./a.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "a.md");
        assert_eq!(t.path_prefix, "");
    }

    #[test]
    fn a_relative_glob_keeps_every_component_after_the_first() {
        let t = table("./src/*/tests/*.rs", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/index/src"));
        assert_eq!(t.glob, "*/tests/*.rs");
        assert_eq!(t.path_prefix, "src");
    }

    #[test]
    fn a_relative_alternation_is_a_glob_not_a_directory() {
        let t = table("./{docs,notes}/*.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "{docs,notes}/*.md");
    }

    #[test]
    fn the_directory_question_is_asked_of_the_absolute_path() {
        let asked = std::cell::RefCell::new(Vec::new());
        let is_dir = |p: &Path| {
            asked.borrow_mut().push(p.to_path_buf());
            true
        };
        table("./docs/nested", &is_dir);
        assert_eq!(asked.into_inner(), [PathBuf::from("/index/docs/nested")]);
    }

    #[test]
    fn a_relative_table_reports_under_its_literal_prefix() {
        assert_eq!(
            table("./docs/nested/*.md", &nothing_is_a_dir).path_prefix,
            "docs/nested"
        );
    }

    #[test]
    fn a_relative_glob_roots_at_its_literal_prefix() {
        let t = table("./docs/*.md", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/index/docs"));
        assert_eq!(t.glob, "*.md");
        assert_eq!(t.path_prefix, "docs");
    }

    #[test]
    fn an_absolute_glob_roots_at_its_literal_prefix() {
        let t = table("/var/log/*.log", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "*.log");
        assert_eq!(t.path_prefix, "/var/log");
    }

    #[test]
    fn an_absolute_directory_lists_one_level() {
        let t = table("/var/log", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn a_parent_relative_directory_lists_one_level() {
        let t = table("../notes", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/notes"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn a_home_relative_directory_lists_one_level() {
        let t = table("~/notes", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/home/u/notes"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn an_absolute_single_file_roots_at_its_parent() {
        let t = table("/var/log/syslog", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "syslog");
        assert_eq!(t.path_prefix, "/var/log");
    }

    #[test]
    fn the_filesystem_root_lists_one_level() {
        let t = table("/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/"));
        assert_eq!(t.glob, "*");
    }

    #[test]
    fn a_deep_glob_keeps_every_component_after_the_first() {
        let t = table("/var/*/logs/*.log", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var"));
        assert_eq!(t.glob, "*/logs/*.log");
    }

    #[test]
    fn a_parent_relative_path_resolves_against_the_index_root() {
        let t = table("../notes/*.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/notes"));
        assert_eq!(t.glob, "*.md");
    }

    #[test]
    fn a_parent_relative_path_folds_repeated_parents() {
        let t = resolve("../../a/*.md", Path::new("/x/y/z"), None, &nothing_is_a_dir);
        assert_eq!(
            t,
            Resolution::Table(PathTable {
                root: PathBuf::from("/x/a"),
                glob: "*.md".to_string(),
                path_prefix: "/x/a".to_string(),
            })
        );
    }

    #[test]
    fn a_parent_relative_path_past_the_filesystem_root_stops_there() {
        let t = resolve("../../*.md", Path::new("/x"), None, &nothing_is_a_dir);
        assert_eq!(
            t,
            Resolution::Table(PathTable {
                root: PathBuf::from("/"),
                glob: "*.md".to_string(),
                path_prefix: "/".to_string(),
            })
        );
    }

    #[test]
    fn a_home_relative_path_resolves_against_the_home_directory() {
        let t = table("~/notes/*.md", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/home/u/notes"));
        assert_eq!(t.glob, "*.md");
        assert_eq!(t.path_prefix, "/home/u/notes");
    }

    #[test]
    fn a_home_relative_path_without_a_home_directory_is_unresolvable() {
        assert_eq!(
            resolve("~/notes", Path::new(ROOT), None, &everything_is_a_dir),
            Resolution::NoHome
        );
    }

    #[test]
    fn a_bare_glob_asks_for_the_dot_slash_form() {
        assert_eq!(resolve_with("**/*.md", &nothing_is_a_dir), Resolution::Hint);
    }

    #[test]
    fn a_plain_identifier_is_not_a_path() {
        assert_eq!(
            resolve_with("users", &nothing_is_a_dir),
            Resolution::NotAPath
        );
    }

    #[test]
    fn a_tilde_without_a_slash_is_not_a_path() {
        assert_eq!(
            resolve_with("~notes", &nothing_is_a_dir),
            Resolution::NotAPath
        );
    }

    #[test]
    fn a_dot_dot_without_a_slash_is_not_a_path() {
        assert_eq!(resolve_with("..", &nothing_is_a_dir), Resolution::NotAPath);
    }

    #[test]
    fn normalize_drops_current_directory_components() {
        assert_eq!(unix_normalize("/a/./b"), "/a/b");
    }

    #[test]
    fn normalize_keeps_a_leading_parent_it_cannot_pop() {
        assert_eq!(unix_normalize("../a"), "../a");
    }

    #[test]
    fn normalize_does_not_pop_a_parent_it_kept() {
        assert_eq!(unix_normalize("../../a"), "../../a");
    }

    fn unix_normalize(path: &str) -> String {
        normalize(Utf8Path::<Utf8UnixEncoding>::new(path))
            .as_str()
            .to_string()
    }

    fn anchor_of(glob: &str, home: Option<&Path>) -> Option<(PathBuf, String)> {
        config_anchor(glob, Path::new("/cfg"), home)
    }

    #[test]
    fn a_config_glob_without_a_prefix_anchors_at_the_config_directory() {
        assert_eq!(
            anchor_of("projects/*/*.jsonl", None),
            Some((PathBuf::from("/cfg"), "projects/*/*.jsonl".to_string()))
        );
    }

    #[test]
    fn a_dot_slash_config_glob_anchors_at_the_config_directory() {
        assert_eq!(
            anchor_of("./*.md", None),
            Some((PathBuf::from("/cfg"), "*.md".to_string()))
        );
    }

    #[test]
    fn an_absolute_config_glob_anchors_at_its_literal_prefix() {
        assert_eq!(
            anchor_of("/var/*/logs/*.log", None),
            Some((PathBuf::from("/var"), "*/logs/*.log".to_string()))
        );
    }

    #[test]
    fn a_home_config_glob_anchors_beneath_the_home_directory() {
        assert_eq!(
            anchor_of("~/.claude/projects/*/*.jsonl", Some(Path::new("/home/u"))),
            Some((
                PathBuf::from("/home/u/.claude/projects"),
                "*/*.jsonl".to_string()
            ))
        );
    }

    #[test]
    fn a_home_config_glob_without_a_home_directory_has_no_anchor() {
        assert_eq!(anchor_of("~/notes/*.md", None), None);
    }

    #[test]
    fn a_parent_config_glob_resolves_against_the_config_directory() {
        assert_eq!(
            anchor_of("../shared/*.md", None),
            Some((PathBuf::from("/shared"), "*.md".to_string()))
        );
    }

    #[test]
    fn a_wholly_literal_absolute_config_glob_anchors_at_its_parent() {
        assert_eq!(
            anchor_of("/var/log/syslog", None),
            Some((PathBuf::from("/var/log"), "syslog".to_string()))
        );
    }

    mod windows {
        use super::*;

        const ROOT: &str = r"C:\index";

        fn resolve_with(name: &str, is_dir: &dyn Fn(&Path) -> bool) -> Resolution {
            resolve_as::<Utf8WindowsEncoding>(
                name,
                Path::new(ROOT),
                Some(Path::new(r"C:\Users\u")),
                is_dir,
            )
        }

        fn table(name: &str, is_dir: &dyn Fn(&Path) -> bool) -> PathTable {
            match resolve_with(name, is_dir) {
                Resolution::Table(t) => t,
                other => panic!("expected a path-table, got {other:?}"),
            }
        }

        #[test]
        fn a_drive_letter_glob_roots_at_its_literal_prefix() {
            let t = table(r"C:\var\log\*.log", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\var\log"));
            assert_eq!(t.glob, "*.log");
            assert_eq!(t.path_prefix, "C:/var/log");
        }

        #[test]
        fn a_drive_letter_path_may_use_forward_slashes() {
            let t = table("C:/var/log/*.log", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\var\log"));
            assert_eq!(t.path_prefix, "C:/var/log");
        }

        #[test]
        fn a_mixed_separator_path_splits_on_both() {
            let t = table(r"C:\Users\u\Temp/docs/*.md", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\Users\u\Temp\docs"));
            assert_eq!(t.glob, "*.md");
            assert_eq!(t.path_prefix, "C:/Users/u/Temp/docs");
        }

        #[test]
        fn a_drive_letter_directory_lists_one_level() {
            let t = table(r"C:\var\log", &everything_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\var\log"));
            assert_eq!(t.glob, "*");
        }

        #[test]
        fn a_backslash_after_a_glob_is_a_star_appended() {
            let t = table(r"C:\var\*\", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\var"));
            assert_eq!(t.glob, "*/*");
        }

        #[test]
        fn a_drive_letter_single_file_roots_at_its_parent() {
            let t = table(r"C:\var\log\syslog", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\var\log"));
            assert_eq!(t.glob, "syslog");
        }

        #[test]
        fn a_drive_root_lists_one_level() {
            let t = table(r"C:\", &everything_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\"));
            assert_eq!(t.glob, "*");
            assert_eq!(t.path_prefix, "C:/");
        }

        #[test]
        fn a_drive_relative_name_is_not_a_path() {
            assert_eq!(
                resolve_with("C:notes", &nothing_is_a_dir),
                Resolution::NotAPath
            );
        }

        #[test]
        fn a_rooted_path_without_a_drive_is_still_absolute() {
            let t = table("/var/log/*.log", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"\var\log"));
            assert_eq!(t.path_prefix, "/var/log");
        }

        #[test]
        fn a_unc_path_roots_at_its_share() {
            let t = table(r"\\server\share\docs\*.md", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"\\server\share\docs"));
            assert_eq!(t.glob, "*.md");
            assert_eq!(t.path_prefix, "//server/share/docs");
        }

        #[test]
        fn a_verbatim_path_keeps_its_backslashes() {
            let t = table(r"\\?\C:\var\log\*.log", &nothing_is_a_dir);
            assert_eq!(t.path_prefix, r"\\?\C:\var\log");
        }

        #[test]
        fn a_parent_relative_path_resolves_against_the_index_root() {
            let t = resolve_as::<Utf8WindowsEncoding>(
                "../../a/*.md",
                Path::new(r"C:\x\y\z"),
                None,
                &nothing_is_a_dir,
            );
            assert_eq!(
                t,
                Resolution::Table(PathTable {
                    root: PathBuf::from(r"C:\x\a"),
                    glob: "*.md".to_string(),
                    path_prefix: "C:/x/a".to_string(),
                })
            );
        }

        #[test]
        fn a_parent_relative_path_past_the_drive_root_stops_there() {
            let t = resolve_as::<Utf8WindowsEncoding>(
                "../../*.md",
                Path::new(r"C:\x"),
                None,
                &nothing_is_a_dir,
            );
            assert_eq!(
                t,
                Resolution::Table(PathTable {
                    root: PathBuf::from(r"C:\"),
                    glob: "*.md".to_string(),
                    path_prefix: "C:/".to_string(),
                })
            );
        }

        #[test]
        fn a_home_relative_path_resolves_against_the_home_directory() {
            let t = table("~/notes/*.md", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\Users\u\notes"));
            assert_eq!(t.glob, "*.md");
            assert_eq!(t.path_prefix, "C:/Users/u/notes");
        }

        #[test]
        fn a_backslash_home_relative_path_resolves_against_the_home_directory() {
            let t = table(r"~\notes\*.md", &nothing_is_a_dir);
            assert_eq!(t.root, Path::new(r"C:\Users\u\notes"));
            assert_eq!(t.glob, "*.md");
        }

        #[test]
        fn a_home_relative_path_without_a_home_directory_is_unresolvable() {
            assert_eq!(
                resolve_as::<Utf8WindowsEncoding>(
                    r"~\notes",
                    Path::new(ROOT),
                    None,
                    &everything_is_a_dir
                ),
                Resolution::NoHome
            );
        }

        #[test]
        fn a_bare_glob_asks_for_the_dot_slash_form() {
            assert_eq!(resolve_with("**/*.md", &nothing_is_a_dir), Resolution::Hint);
        }

        #[test]
        fn a_plain_identifier_is_not_a_path() {
            assert_eq!(
                resolve_with("users", &nothing_is_a_dir),
                Resolution::NotAPath
            );
        }
    }

    #[test]
    fn a_backslash_after_a_tilde_is_not_a_unix_path() {
        assert_eq!(
            resolve_as::<Utf8UnixEncoding>(
                r"~\notes",
                Path::new(ROOT),
                Some(Path::new("/home/u")),
                &nothing_is_a_dir
            ),
            Resolution::NotAPath
        );
    }
}
