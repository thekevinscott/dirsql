//! Turning the string a user writes where a table name goes into a concrete
//! scan: a directory to walk, a glob relative to it, and the base the `path`
//! column is reported against.
//!
//! Every decision here is a pure function of the written string plus one
//! filesystem question (*is this a directory?*), which is injected so the
//! rules can be tested without a filesystem.

use std::path::{Component, Path, PathBuf};

/// Directories a path-table scan skips, at any depth. They are skipped only
/// *beneath* the literal part of the path you write, so naming one explicitly
/// still scans it.
pub const DEFAULT_IGNORES: [&str; 2] = ["**/node_modules/**", "**/.git/**"];

/// The glob a directory expands to: recursion is the default, and the
/// non-recursive form is spelled explicitly as `*`.
const RECURSIVE_GLOB: &str = "**/*";

/// A resolved path-table: what to walk, what to match, and what to report.
#[derive(Debug, PartialEq)]
pub struct PathTable {
    /// Directory the scan walks.
    pub root: PathBuf,
    /// Glob matched against paths relative to [`root`](Self::root).
    pub glob: String,
    /// Prepended to each matched relative path before the stat columns are
    /// computed. Empty for index-root-relative tables, which report relative
    /// paths; the scan root for the rest, which report absolute ones.
    pub path_prefix: String,
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

/// Whether `name` contains a character that makes it a glob rather than a
/// literal path.
fn has_glob_metacharacter(name: &str) -> bool {
    name.contains(['*', '?', '['])
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
    if let Some(rest) = name.strip_prefix("./") {
        return Resolution::Table(PathTable {
            root: index_root.to_path_buf(),
            glob: relative_glob(rest, &|rel| is_dir(&index_root.join(rel))),
            path_prefix: String::new(),
        });
    }

    match absolute_target(name, index_root, home) {
        Some(Some(target)) => Resolution::Table(split_absolute(&target, is_dir)),
        Some(None) => Resolution::NoHome,
        None if has_glob_metacharacter(name) => Resolution::Hint,
        None => Resolution::NotAPath,
    }
}

/// The glob a `./`-relative path expands to, relative to the index root.
fn relative_glob(rest: &str, is_dir: &dyn Fn(&Path) -> bool) -> String {
    let trimmed = rest.trim_end_matches('/');

    if trimmed.is_empty() {
        return RECURSIVE_GLOB.to_string();
    }
    if has_glob_metacharacter(trimmed) {
        return trimmed.to_string();
    }
    if is_dir(Path::new(trimmed)) {
        return format!("{trimmed}/{RECURSIVE_GLOB}");
    }
    trimmed.to_string()
}

/// The absolute path a non-`./` path-table names, with `.` and `..` folded out.
///
/// `None` means the name is not a path at all. `Some(None)` means it is a `~/`
/// path but no home directory could be found.
fn absolute_target(name: &str, index_root: &Path, home: Option<&Path>) -> Option<Option<PathBuf>> {
    if let Some(rest) = name.strip_prefix("~/") {
        return Some(home.map(|h| normalize(&h.join(rest))));
    }
    if name.starts_with("../") {
        return Some(Some(normalize(&index_root.join(name))));
    }
    if name.starts_with('/') {
        return Some(Some(normalize(Path::new(name))));
    }
    None
}

/// Fold `.` and `..` out of `path` lexically. Purely textual: a `..` is not
/// resolved through a symlink, which keeps the answer a function of the string
/// the user wrote.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            // A `..` that cannot pop is kept on a relative path (it still
            // means something) but dropped at the filesystem root, which has
            // no parent to climb to.
            Component::ParentDir => {
                if !out.pop() && !out.has_root() {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Split an absolute path-table target into the directory to walk and the glob
/// to match beneath it. A wholly literal target is a directory (scan it
/// recursively) or a single file (match exactly that name).
fn split_absolute(target: &Path, is_dir: &dyn Fn(&Path) -> bool) -> PathTable {
    let (literal, rest) = split_at_first_glob(target);

    if !rest.is_empty() {
        return table_at(literal, rest);
    }
    if is_dir(&literal) {
        return table_at(literal, RECURSIVE_GLOB.to_string());
    }

    let name = literal
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| RECURSIVE_GLOB.to_string());
    let parent = literal.parent().unwrap_or(&literal).to_path_buf();
    table_at(parent, name)
}

/// A path-table rooted at `root`, reporting absolute paths.
fn table_at(root: PathBuf, glob: String) -> PathTable {
    PathTable {
        path_prefix: root.to_string_lossy().into_owned(),
        root,
        glob,
    }
}

/// Split `target` at its first glob-bearing component: the literal directory
/// chain ahead of it, and the rest as a `/`-joined glob.
fn split_at_first_glob(target: &Path) -> (PathBuf, String) {
    let mut literal = PathBuf::new();
    let mut rest: Vec<String> = Vec::new();

    for component in target.components() {
        let text = component.as_os_str().to_string_lossy().into_owned();
        if rest.is_empty() && !has_glob_metacharacter(&text) {
            literal.push(component);
        } else {
            rest.push(text);
        }
    }

    (literal, rest.join("/"))
}

/// The leading literal directories of `glob` — the part the user named
/// outright. Skip rules are evaluated below this, so writing
/// `'./node_modules'` scans a directory the defaults would otherwise skip.
pub fn ignore_base(glob: &str) -> PathBuf {
    let mut base = PathBuf::new();
    for segment in glob.split('/') {
        if has_glob_metacharacter(segment) {
            break;
        }
        base.push(segment);
    }
    // A wholly literal glob names one file; its own directory chain is the
    // named part, so nothing is left to check.
    base
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

    #[test]
    fn default_ignores_cover_vcs_and_dependency_directories() {
        assert_eq!(DEFAULT_IGNORES, ["**/node_modules/**", "**/.git/**"]);
    }

    #[test]
    fn has_glob_metacharacter_spots_each_metacharacter() {
        assert!(has_glob_metacharacter("a*"));
        assert!(has_glob_metacharacter("a?"));
        assert!(has_glob_metacharacter("a[bc]"));
    }

    #[test]
    fn has_glob_metacharacter_is_false_for_a_literal_path() {
        assert!(!has_glob_metacharacter("docs/a.md"));
    }

    #[test]
    fn a_bare_dot_slash_scans_the_index_root_recursively() {
        let t = table("./", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new(ROOT));
        assert_eq!(t.glob, "**/*");
        assert_eq!(t.path_prefix, "");
    }

    #[test]
    fn a_relative_directory_expands_recursively() {
        assert_eq!(table("./docs", &everything_is_a_dir).glob, "docs/**/*");
    }

    #[test]
    fn a_relative_directory_with_a_trailing_slash_expands_recursively() {
        assert_eq!(table("./docs/", &everything_is_a_dir).glob, "docs/**/*");
    }

    #[test]
    fn a_relative_single_file_matches_only_itself() {
        assert_eq!(table("./docs/a.md", &nothing_is_a_dir).glob, "docs/a.md");
    }

    #[test]
    fn a_relative_glob_is_used_as_written() {
        assert_eq!(table("./docs/*.md", &everything_is_a_dir).glob, "docs/*.md");
    }

    #[test]
    fn an_explicit_star_is_left_non_recursive() {
        assert_eq!(table("./*", &everything_is_a_dir).glob, "*");
    }

    #[test]
    fn a_relative_table_reports_relative_paths() {
        assert_eq!(table("./docs/*.md", &nothing_is_a_dir).path_prefix, "");
    }

    #[test]
    fn an_absolute_glob_roots_at_its_literal_prefix() {
        let t = table("/var/log/*.log", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "*.log");
        assert_eq!(t.path_prefix, "/var/log");
    }

    #[test]
    fn an_absolute_directory_expands_recursively() {
        let t = table("/var/log", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "**/*");
    }

    #[test]
    fn an_absolute_single_file_roots_at_its_parent() {
        let t = table("/var/log/syslog", &nothing_is_a_dir);
        assert_eq!(t.root, Path::new("/var/log"));
        assert_eq!(t.glob, "syslog");
        assert_eq!(t.path_prefix, "/var/log");
    }

    #[test]
    fn the_filesystem_root_scans_everything() {
        let t = table("/", &everything_is_a_dir);
        assert_eq!(t.root, Path::new("/"));
        assert_eq!(t.glob, "**/*");
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
        assert_eq!(normalize(Path::new("/a/./b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn normalize_keeps_a_leading_parent_it_cannot_pop() {
        assert_eq!(normalize(Path::new("../a")), PathBuf::from("../a"));
    }

    #[test]
    fn ignore_base_is_empty_for_a_leading_glob() {
        assert_eq!(ignore_base("**/*"), PathBuf::new());
    }

    #[test]
    fn ignore_base_is_the_literal_prefix_of_a_glob() {
        assert_eq!(
            ignore_base("node_modules/**/*"),
            PathBuf::from("node_modules")
        );
    }

    #[test]
    fn ignore_base_is_the_whole_of_a_literal_glob() {
        assert_eq!(ignore_base("docs/a.md"), PathBuf::from("docs/a.md"));
    }
}
