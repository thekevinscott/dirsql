//! Path-tables over symlinks, held to what `bash -O globstar -O nullglob`
//! lists for the same pattern over the same tree. Real filesystem, real
//! SQLite, SDK public API.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// `link.md -> top.md`, `linkdir -> real`, `real/loop -> .` (a cycle) and a
/// dangling `broken.md`.
fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    let at = |rel: &str| root.path().join(rel);
    fs::create_dir_all(at("real/inner")).unwrap();
    fs::write(at("top.md"), "top").unwrap();
    fs::write(at("real/r.md"), "r").unwrap();
    fs::write(at("real/inner/ri.md"), "ri").unwrap();
    symlink("top.md", at("link.md")).unwrap();
    symlink("real", at("linkdir")).unwrap();
    symlink(".", at("real/loop")).unwrap();
    symlink("missing.md", at("broken.md")).unwrap();
    root
}

fn paths(table: &str) -> Vec<String> {
    let root = fixture();
    let db = DirSQL::new(root.path(), vec![]).unwrap();
    let mut out: Vec<String> = db
        .query(&format!("SELECT path FROM '{table}'"))
        .unwrap()
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
fn a_star_lists_a_symlinked_file_but_not_a_broken_link() {
    assert_eq!(paths("./*"), vec!["link.md", "top.md"]);
}

#[test]
fn a_symlinked_file_named_exactly_is_listed() {
    assert_eq!(paths("./link.md"), vec!["link.md"]);
}

#[test]
fn a_broken_link_named_exactly_lists_nothing() {
    assert!(paths("./broken.md").is_empty());
}

#[test]
fn a_double_star_lists_symlinked_files_but_never_enters_a_symlinked_directory() {
    assert_eq!(
        paths("./**"),
        vec!["link.md", "real/inner/ri.md", "real/r.md", "top.md"]
    );
}

#[test]
fn a_star_component_enters_a_symlinked_directory() {
    assert_eq!(paths("./*/*"), vec!["linkdir/r.md", "real/r.md"]);
}

#[test]
fn a_double_star_may_end_on_a_symlinked_directory_the_next_component_enters() {
    assert_eq!(
        paths("./**/*.md"),
        vec![
            "link.md",
            "linkdir/r.md",
            "real/inner/ri.md",
            "real/loop/r.md",
            "real/r.md",
            "top.md"
        ]
    );
}

#[test]
fn a_literal_symlinked_directory_is_walked_by_the_double_star_after_it() {
    assert_eq!(
        paths("./linkdir/**"),
        vec!["linkdir/inner/ri.md", "linkdir/r.md"]
    );
}

#[test]
fn a_symlink_cycle_is_followed_once_per_star_component() {
    assert_eq!(
        paths("./*/*/*/*"),
        vec![
            "linkdir/loop/inner/ri.md",
            "linkdir/loop/loop/r.md",
            "real/loop/inner/ri.md",
            "real/loop/loop/r.md"
        ]
    );
}
