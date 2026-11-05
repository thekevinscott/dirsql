//! Integration tests for `$GIT_DIR/info/exclude` in path-table scans: inside
//! a git repo it hides what it names, below any `.gitignore` in precedence,
//! as git does, including in a linked worktree or submodule whose `.git` is a
//! file. Real filesystem, real SQLite, SDK public API.

use std::fs;
use std::path::Path;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// A repo whose `.git/info/exclude` holds `exclude`, with `a.txt`, `b.txt`
/// and `sub/c.txt`.
fn repo(exclude: &str) -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join(".git/info")).unwrap();
    fs::write(root.path().join(".git/info/exclude"), exclude).unwrap();
    populate(root.path());
    root
}

fn populate(dir: &Path) {
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("a.txt"), "").unwrap();
    fs::write(dir.join("b.txt"), "").unwrap();
    fs::write(dir.join("sub/c.txt"), "").unwrap();
}

fn scan(root: &Path, glob: &str) -> Vec<String> {
    let db = DirSQL::builder().root(root).build().unwrap();
    let rows = db
        .query(&format!("SELECT path FROM '{glob}' ORDER BY path"))
        .unwrap();
    paths(&rows)
}

fn paths(rows: &[Row]) -> Vec<String> {
    rows.iter()
        .map(|r| match r.get("path") {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("path was not text: {other:?}"),
        })
        .collect()
}

#[test]
fn info_exclude_hides_what_it_names_at_any_depth() {
    let root = repo("a.txt\nc.txt\n");

    assert_eq!(scan(root.path(), "./**"), vec!["b.txt"]);
}

#[test]
fn info_exclude_applies_to_a_scan_started_below_the_repo_root() {
    let root = repo("c.txt\n");

    assert_eq!(scan(root.path(), "./sub/*"), Vec::<String>::new());
}

#[test]
fn info_exclude_applies_when_the_index_root_is_below_the_repo_root() {
    let root = repo("c.txt\n");

    assert_eq!(scan(&root.path().join("sub"), "./*"), Vec::<String>::new());
}

#[test]
fn a_gitignore_negation_outranks_info_exclude() {
    let root = repo("*.txt\n");
    fs::write(root.path().join(".gitignore"), "!a.txt\n").unwrap();

    assert_eq!(scan(root.path(), "./*"), vec!["a.txt"]);
}

#[test]
fn a_linked_worktree_reads_the_common_dir_info_exclude() {
    let main = repo("a.txt\n");
    let gitdir = main.path().join(".git/worktrees/linked");
    fs::create_dir_all(&gitdir).unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    let linked = TempDir::new().unwrap();
    fs::write(
        linked.path().join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    populate(linked.path());

    assert_eq!(scan(linked.path(), "./*"), vec!["b.txt"]);
}

#[test]
fn a_submodule_reads_its_own_git_dir_info_exclude() {
    let outer = TempDir::new().unwrap();
    let gitdir = outer.path().join(".git/modules/inner");
    fs::create_dir_all(gitdir.join("info")).unwrap();
    fs::write(gitdir.join("info/exclude"), "b.txt\n").unwrap();
    let inner = outer.path().join("inner");
    fs::create_dir_all(&inner).unwrap();
    fs::write(inner.join(".git"), "gitdir: ../.git/modules/inner\n").unwrap();
    populate(&inner);

    assert_eq!(scan(&inner, "./*"), vec!["a.txt"]);
}
