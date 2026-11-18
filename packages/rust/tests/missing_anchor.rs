//! A table whose own directory does not exist fails the build, naming the
//! table and the directory. A missing subdirectory under an existing one does
//! not.

use dirsql::{DirSQL, Table};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn table(name: &str, glob: &str) -> Table {
    Table::new(
        name,
        format!("CREATE TABLE {name} (x TEXT)"),
        glob,
        |_| vec![],
    )
}

fn build_error(result: dirsql::Result<DirSQL>) -> String {
    match result {
        Ok(_) => panic!("the build succeeded"),
        Err(e) => e.to_string(),
    }
}

fn write_config(dir: &Path, glob: &str) -> std::path::PathBuf {
    let path = dir.join(".dirsql.toml");
    fs::write(
        &path,
        format!(
            "[[table]]\nname = \"present\"\nddl = \"CREATE TABLE present (x TEXT)\"\nglob = \"*.json\"\non-file = \"true\"\n\n\
             [[table]]\nname = \"later\"\nddl = \"CREATE TABLE later (x TEXT)\"\nglob = \"{glob}\"\non-file = \"true\"\n"
        ),
    )
    .unwrap();
    path
}

#[test]
fn an_anchored_table_whose_directory_is_missing_fails_the_build() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("absent");

    let err = build_error(DirSQL::new(
        dir.path(),
        vec![
            table("present", "*.json"),
            table("later", "*.json").anchored(&missing),
        ],
    ));

    assert_eq!(
        err,
        format!(
            "table 'later': its directory {} does not exist",
            missing.display()
        )
    );
}

#[test]
fn a_config_glob_above_the_config_whose_directory_is_missing_fails_the_build() {
    let dir = TempDir::new().unwrap();
    let cfg_dir = dir.path().join("cfg");
    fs::create_dir(&cfg_dir).unwrap();
    let cfg = write_config(&cfg_dir, "../absent/*.json");

    let err = build_error(DirSQL::builder().root(&cfg_dir).config(&cfg).build());

    assert!(err.starts_with("table 'later': its directory "), "{err}");
    assert!(err.ends_with("absent does not exist"), "{err}");
}

#[test]
fn a_missing_subdirectory_under_an_existing_anchor_still_builds() {
    let dir = TempDir::new().unwrap();

    let built = DirSQL::new(
        dir.path(),
        vec![table("later", "absent/*.json").anchored(dir.path())],
    );

    assert!(built.is_ok());
}
