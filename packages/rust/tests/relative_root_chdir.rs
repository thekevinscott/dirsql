//! A relative `root` names the directory it named when the instance was
//! built: a later `chdir` must not re-resolve it for path-tables.

use dirsql::{DirSQL, Value};
use std::fs;

#[test]
fn path_tables_keep_a_relative_root_after_chdir() {
    let dir = tempfile::TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(base.join("tree").join("sub")).unwrap();
    fs::write(base.join("tree").join("a.json"), "{}\n").unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base).unwrap();

    let db = DirSQL::builder().root("tree").build().unwrap();
    std::env::set_current_dir(base.join("tree").join("sub")).unwrap();
    let rows = db.query("SELECT path FROM './**'");
    std::env::set_current_dir(&original).unwrap();

    let paths: Vec<Value> = rows
        .unwrap()
        .into_iter()
        .map(|row| row["path"].clone())
        .collect();
    assert_eq!(paths, vec![Value::Text("a.json".to_string())]);
}
