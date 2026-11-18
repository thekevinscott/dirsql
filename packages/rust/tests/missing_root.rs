//! An index root that does not exist fails the build, naming the root.

use dirsql::{DirSQL, Table};
use tempfile::TempDir;

fn table() -> Table {
    Table::new("t", "CREATE TABLE t (x TEXT)", "*.json", |_| vec![])
}

fn build_error(result: dirsql::Result<DirSQL>) -> String {
    match result {
        Ok(_) => panic!("the build succeeded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn a_missing_root_fails_the_build() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("absent");

    let err = build_error(DirSQL::new(&missing, vec![table()]));

    assert_eq!(
        err,
        format!("root directory {} does not exist", missing.display())
    );
}

#[test]
fn a_missing_root_fails_the_build_with_no_tables() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("absent");

    let err = build_error(DirSQL::builder().root(&missing).build());

    assert_eq!(
        err,
        format!("root directory {} does not exist", missing.display())
    );
}
