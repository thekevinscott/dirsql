//! Integration tests: a folder a glob reaches that exists but cannot be listed
//! fails the scan with an error naming it, where a missing folder is no rows.
//! Real filesystem permissions, so Unix only.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use dirsql::{DirSQL, Value};
use tempfile::TempDir;

/// Make `dir` traversable but not listable. Returns false when the process
/// bypasses permission checks (root), since the test's premise cannot hold.
fn make_unlistable(dir: &Path) -> bool {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o311)).unwrap();
    fs::read_dir(dir).is_err()
}

fn make_listable(dir: &Path) {
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
}

/// `root/data/open/a.ndjson` and `root/data/locked/b.ndjson`, `locked` made
/// unlistable; `None` when it cannot be.
fn tree() -> Option<(TempDir, std::path::PathBuf)> {
    let root = TempDir::new().unwrap();
    let open = root.path().join("data/open");
    let locked = root.path().join("data/locked");
    fs::create_dir_all(&open).unwrap();
    fs::create_dir_all(&locked).unwrap();
    fs::write(open.join("a.ndjson"), "{\"k\":\"a\"}\n").unwrap();
    fs::write(locked.join("b.ndjson"), "{\"k\":\"b\"}\n").unwrap();
    if !make_unlistable(&locked) {
        make_listable(&locked);
        return None;
    }
    Some((root, locked))
}

fn parsed(root: &Path) -> DirSQL {
    DirSQL::builder()
        .root(root)
        .path_table_parser("cat")
        .build()
        .unwrap()
}

fn assert_names(message: &str, locked: &Path) {
    assert!(
        message.contains(&locked.display().to_string()),
        "the error must name the unlistable folder: {message}"
    );
}

#[test]
fn a_parsed_path_table_over_an_unlistable_folder_fails_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = parsed(root.path()).query("SELECT * FROM './data/**/*.ndjson'");
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_parsed_literal_path_inside_an_unlistable_folder_fails_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = parsed(root.path()).query("SELECT * FROM './data/locked/b.ndjson'");
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_parsed_path_table_over_a_missing_folder_is_still_a_listable_one() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let rows = parsed(root.path()).query("SELECT k FROM './data/{open,missing}/*.ndjson'");
    make_listable(&locked);

    assert_eq!(rows.unwrap()[0]["k"], Value::Text("a".into()));
}

fn config(root: &Path, glob: &str) -> std::path::PathBuf {
    let config = root.join(".dirsql.toml");
    fs::write(
        &config,
        format!(
            "[[table]]\nname = \"recs\"\nglob = \"{glob}\"\non-file = \"cat\"\n\
             ddl = \"CREATE TABLE recs (k TEXT)\"\n"
        ),
    )
    .unwrap();
    config
}

fn config_build(root: &Path, glob: &str) -> dirsql::Result<DirSQL> {
    DirSQL::builder()
        .root(root)
        .config(config(root, glob))
        .build()
}

#[test]
fn a_config_table_over_an_unlistable_folder_fails_its_build_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = config_build(root.path(), "data/**/*.ndjson");
    make_listable(&locked);

    let message = result
        .err()
        .expect("an unlistable folder must fail the build")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_config_table_over_a_missing_folder_builds_with_no_rows() {
    let root = TempDir::new().unwrap();
    let db = config_build(root.path(), "missing/*.ndjson").unwrap();

    let rows = db.query("SELECT COUNT(*) AS n FROM recs").unwrap();
    assert_eq!(rows[0]["n"], Value::Integer(0));
}

#[test]
fn a_config_table_through_a_link_to_an_unlistable_folder_fails_its_build_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let index = root.path().join("index");
    fs::create_dir(&index).unwrap();
    std::os::unix::fs::symlink(&locked, index.join("link")).unwrap();
    let result = DirSQL::builder()
        .root(&index)
        .config(config(&index, "link/*.ndjson"))
        .build();
    make_listable(&locked);

    let message = result
        .err()
        .expect("an unlistable folder must fail the build")
        .to_string();
    assert_names(&message, &index.join("link"));
}

fn streamed(root: &Path) -> DirSQL {
    DirSQL::builder().root(root).build().unwrap()
}

#[test]
fn a_path_table_over_an_unlistable_folder_fails_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = streamed(root.path()).query("SELECT path FROM './data/**/*.ndjson'");
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_path_table_read_straight_through_fails_naming_an_unlistable_folder() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = streamed(root.path()).query_each(
        "SELECT * FROM './data/**/*.ndjson'",
        |_| 0,
        |n, _| *n += 1,
    );
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_path_table_lookup_fails_naming_an_unlistable_folder() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = streamed(root.path())
        .query("SELECT path FROM './data/**/*.ndjson' WHERE path = 'data/open/a.ndjson'");
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_path_table_literal_path_inside_an_unlistable_folder_fails_naming_it() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let result = streamed(root.path()).query("SELECT path FROM './data/locked/b.ndjson'");
    make_listable(&locked);

    let message = result
        .expect_err("an unlistable folder must fail the query")
        .to_string();
    assert_names(&message, &locked);
}

#[test]
fn a_path_table_over_a_missing_folder_is_still_a_listable_one() {
    let Some((root, locked)) = tree() else {
        return;
    };
    let rows = streamed(root.path()).query("SELECT path FROM './data/{open,missing}/*.ndjson'");
    make_listable(&locked);

    assert_eq!(
        rows.unwrap()[0]["path"],
        Value::Text("data/open/a.ndjson".into())
    );
}
