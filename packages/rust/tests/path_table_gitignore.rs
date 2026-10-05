//! Integration tests for gitignore-by-default in path-table scans: a
//! `.gitignore` anywhere in the tree prunes the files it names below its own
//! directory, hierarchically, like fd/ripgrep — with no `.git` directory
//! required. Dot-named entries are hidden unless the path spells them.
//! Real filesystem, real SQLite, SDK public API.

use std::fs;

use dirsql::{DirSQL, Row, Value};
use tempfile::TempDir;

/// A tree with a root `.gitignore`, a nested one, ignored and kept files,
/// hidden files, and a `node_modules` for the built-in floor.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("dist")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("sub")).unwrap();
    fs::create_dir_all(root.path().join(".hidden")).unwrap();
    fs::write(root.path().join(".gitignore"), "dist/\n*.log\n!keep.log\n").unwrap();
    fs::write(root.path().join("dist/bundle.js"), "js").unwrap();
    fs::write(root.path().join("src/app.js"), "js").unwrap();
    fs::write(root.path().join("debug.log"), "log").unwrap();
    fs::write(root.path().join("keep.log"), "log").unwrap();
    fs::write(root.path().join("sub/.gitignore"), "*.tmp\n").unwrap();
    fs::write(root.path().join("sub/scratch.tmp"), "tmp").unwrap();
    fs::write(root.path().join("sub/notes.md"), "note").unwrap();
    fs::write(root.path().join(".hidden/secret.txt"), "shh").unwrap();
    fs::write(root.path().join(".env"), "X=1").unwrap();
    root
}

fn open(root: &TempDir) -> DirSQL {
    DirSQL::builder().root(root.path()).build().unwrap()
}

fn paths(rows: &[Row]) -> Vec<String> {
    let mut out: Vec<String> = rows
        .iter()
        .map(|r| match r.get("path") {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("path was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_root_gitignore_excludes_its_matches_from_a_path_table_scan() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './**'").unwrap();
    let scanned = paths(&rows);

    assert!(
        !scanned.contains(&"dist/bundle.js".to_string()),
        "a `dist/` gitignore rule must prune the dist subtree, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&"debug.log".to_string()),
        "a `*.log` gitignore rule must exclude matching files, got: {scanned:?}"
    );
    assert!(
        scanned.contains(&"src/app.js".to_string()),
        "unignored files must still scan, got: {scanned:?}"
    );
}

#[test]
fn a_gitignore_negation_keeps_the_whitelisted_file() {
    let root = fixture();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        scanned.contains(&"keep.log".to_string()),
        "`!keep.log` must override the `*.log` rule, got: {scanned:?}"
    );
}

#[test]
fn a_nested_gitignore_applies_below_its_own_directory() {
    let root = fixture();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        !scanned.contains(&"sub/scratch.tmp".to_string()),
        "sub/.gitignore's `*.tmp` must exclude files under sub/, got: {scanned:?}"
    );
    assert!(
        scanned.contains(&"sub/notes.md".to_string()),
        "files the nested gitignore does not name must still scan, got: {scanned:?}"
    );
}

#[test]
fn a_nested_gitignore_does_not_reach_outside_its_directory() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("sub")).unwrap();
    fs::write(root.path().join("sub/.gitignore"), "*.md\n").unwrap();
    fs::write(root.path().join("sub/inside.md"), "in").unwrap();
    fs::write(root.path().join("outside.md"), "out").unwrap();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        scanned.contains(&"outside.md".to_string()),
        "a nested gitignore applies below its directory only, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&"sub/inside.md".to_string()),
        "the nested rule must still apply beneath it, got: {scanned:?}"
    );
}

#[test]
fn dot_named_entries_are_hidden_from_a_recursive_scan() {
    let root = fixture();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        !scanned.contains(&".env".to_string()),
        "a dotfile the path does not spell is hidden, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&".hidden/secret.txt".to_string()),
        "a dot-directory the path does not spell is not walked, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&".gitignore".to_string()),
        "the .gitignore file itself is a dotfile, got: {scanned:?}"
    );
}

#[test]
fn dot_named_entries_stay_hidden_without_gitignore_respect() {
    let root = fixture();
    let db = DirSQL::builder()
        .root(root.path())
        .no_ignore(true)
        .build()
        .unwrap();

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        scanned.contains(&"dist/bundle.js".to_string()),
        "no_ignore restores gitignored files, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&".env".to_string()),
        "dotfile hiding is independent of no_ignore, got: {scanned:?}"
    );
}

#[test]
fn spelling_a_dot_directory_scans_it() {
    let root = fixture();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './.hidden/*'").unwrap());

    assert_eq!(scanned, vec![".hidden/secret.txt"]);
}

#[test]
fn a_path_table_rooted_inside_a_gitignored_directory_still_scans() {
    let root = fixture();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './dist'").unwrap();

    assert_eq!(
        paths(&rows),
        vec!["dist/bundle.js"],
        "naming a gitignored directory outright must scan it"
    );
}

#[test]
fn no_ignore_restores_gitignored_files_but_keeps_the_built_in_floor() {
    let root = fixture();
    fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    fs::write(root.path().join("node_modules/pkg/index.js"), "js").unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .no_ignore(true)
        .build()
        .unwrap();

    let scanned = paths(&db.query("SELECT path FROM './**'").unwrap());

    assert!(
        scanned.contains(&"dist/bundle.js".to_string()),
        "no_ignore must disable gitignore respect, got: {scanned:?}"
    );
    assert!(
        scanned.contains(&"debug.log".to_string()),
        "file-level gitignore rules are off too, got: {scanned:?}"
    );
    assert!(
        !scanned.contains(&"node_modules/pkg/index.js".to_string()),
        "the built-in defaults still apply under no_ignore, got: {scanned:?}"
    );
}

#[test]
fn a_scoped_glob_still_honors_gitignore_rules_beneath_its_base() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/.gitignore"), "draft.md\n").unwrap();
    fs::write(root.path().join("docs/draft.md"), "d").unwrap();
    fs::write(root.path().join("docs/final.md"), "f").unwrap();
    let db = open(&root);

    let rows = db.query("SELECT path FROM './docs/*.md'").unwrap();

    assert_eq!(
        paths(&rows),
        vec!["docs/final.md"],
        "a gitignore at the named base still filters below it"
    );
}

#[test]
fn a_gitignore_above_the_named_base_does_not_reach_beneath_it() {
    let root = fixture();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/a.md"), "a").unwrap();
    fs::write(root.path().join("docs/trace.log"), "log").unwrap();
    let db = open(&root);

    let scanned = paths(&db.query("SELECT path FROM './docs/*'").unwrap());

    assert_eq!(
        scanned,
        vec!["docs/a.md", "docs/trace.log"],
        "the root `*.log` rule is above the named base, so it does not apply there"
    );
}
