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

#[test]
fn a_dot_slash_glob_with_repeated_separators_anchors_at_the_config_directory() {
    let cfg_dir = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(cfg_dir.path().join("a")).unwrap();
    fs::write(cfg_dir.path().join("a/x.json"), "x").unwrap();
    let cfg = write_config(cfg_dir.path(), ".//a/*.json", "");

    let db = DirSQL::builder().config(&cfg).build().unwrap();

    assert_eq!(paths(&db), vec!["a/x.json".to_string()]);
}

/// `y/link -> real/sub`, so `y/link/..` is `real` to the filesystem and `y`
/// to a lexical fold. Each `f.md` says which one was read.
#[cfg(unix)]
fn contents_through_link(glob_under: impl Fn(&Path) -> String) -> Vec<String> {
    let dir = tempfile::TempDir::new().unwrap();
    let at = |rel: &str| dir.path().join(rel);
    fs::create_dir_all(at("cfg")).unwrap();
    fs::create_dir_all(at("real/sub")).unwrap();
    fs::create_dir_all(at("y")).unwrap();
    fs::write(at("real/f.md"), "PHYSICAL").unwrap();
    fs::write(at("y/f.md"), "LEXICAL").unwrap();
    std::os::unix::fs::symlink(at("real/sub"), at("y/link")).unwrap();
    let cfg = at("cfg/.dirsql.toml");
    fs::write(
        &cfg,
        format!(
            "[[table]]\nname = \"files\"\nddl = \"CREATE TABLE files (content TEXT)\"\nglob = '{}'\non-file = '''sh -c 'for p; do printf \"{{\\\"content\\\":\\\"%s\\\"}}\\n\" \"$(cat \"$p\")\"; done' sh'''\n",
            glob_under(dir.path())
        ),
    )
    .unwrap();

    let db = DirSQL::builder().config(&cfg).build().unwrap();
    db.query("SELECT content FROM files")
        .unwrap()
        .iter()
        .map(|row| match &row["content"] {
            Value::Text(text) => text.clone(),
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn an_absolute_glob_resolves_a_parent_through_a_symlinked_directory_physically() {
    assert_eq!(
        contents_through_link(|d| format!("{}/y/link/../*.md", d.display())),
        vec!["PHYSICAL".to_string()]
    );
}

#[cfg(unix)]
#[test]
fn a_parent_relative_glob_resolves_a_parent_through_a_symlinked_directory_physically() {
    assert_eq!(
        contents_through_link(|_| "../y/link/../*.md".to_string()),
        vec!["PHYSICAL".to_string()]
    );
}
