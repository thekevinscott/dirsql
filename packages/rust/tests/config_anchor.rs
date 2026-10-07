//! A config `[[table]] glob` anchors at the directory of the config file that
//! wrote it, whatever the index root is.

use dirsql::{DirSQL, Value};
use std::fs;
use std::path::Path;

const HOOK: &str = r#"on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "{\"path\":\"%s\"}\n" "$rel"; done' sh {root}'''"#;

fn write_config(dir: &Path, glob: &str, extra: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(
        &path,
        format!(
            "{extra}\n[[table]]\nname = \"files\"\nddl = \"CREATE TABLE files (path TEXT)\"\nglob = '{glob}'\n{HOOK}\n"
        ),
    )
    .unwrap();
    path
}

fn paths(db: &DirSQL) -> Vec<String> {
    let mut paths: Vec<String> = db
        .query("SELECT path FROM files")
        .unwrap()
        .iter()
        .map(|row| match &row["path"] {
            Value::Text(text) => text.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    paths.sort();
    paths
}

#[test]
fn a_relative_glob_anchors_at_the_config_directory_not_the_index_root() {
    let cfg_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(cfg_dir.path().join("projects/p")).unwrap();
    fs::write(cfg_dir.path().join("projects/p/a.txt"), "x").unwrap();
    let root_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(root_dir.path().join("projects/p")).unwrap();
    fs::write(root_dir.path().join("projects/p/decoy.txt"), "x").unwrap();
    let cfg = write_config(cfg_dir.path(), "projects/*/*.txt", "");

    let db = DirSQL::builder()
        .root(root_dir.path())
        .config(&cfg)
        .build()
        .unwrap();

    assert_eq!(paths(&db), vec!["projects/p/a.txt".to_string()]);
}

#[test]
fn an_absolute_glob_anchors_at_its_literal_prefix() {
    let cfg_dir = tempfile::TempDir::new().unwrap();
    let data = tempfile::TempDir::new().unwrap();
    let data = data.path().to_path_buf();
    fs::create_dir_all(data.join("sub")).unwrap();
    fs::write(data.join("sub/b.txt"), "x").unwrap();
    fs::write(cfg_dir.path().join("decoy.txt"), "x").unwrap();
    let glob = format!("{}/*/*.txt", data.display()).replace('\\', "/");
    let cfg = write_config(cfg_dir.path(), &glob, "");

    let db = DirSQL::builder().config(&cfg).build().unwrap();

    assert_eq!(paths(&db), vec!["sub/b.txt".to_string()]);
}

#[test]
fn a_config_ignore_is_relative_to_the_config_anchor() {
    let cfg_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(cfg_dir.path().join("skip")).unwrap();
    fs::write(cfg_dir.path().join("skip/a.txt"), "x").unwrap();
    fs::write(cfg_dir.path().join("keep.txt"), "x").unwrap();
    let root_dir = tempfile::TempDir::new().unwrap();
    let cfg = write_config(
        cfg_dir.path(),
        "**/*.txt",
        "[dirsql]\nignore = [\"skip/**\"]",
    );

    let db = DirSQL::builder()
        .root(root_dir.path())
        .config(&cfg)
        .build()
        .unwrap();

    assert_eq!(paths(&db), vec!["keep.txt".to_string()]);
}
