//! A `.gitignore` edit while watching brings the directories it un-ignores
//! into the live index. Real watcher, real filesystem, SDK public API.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{fs, thread};

use dirsql::{DirSQL, Table, Value};
use tempfile::TempDir;

fn names(db: &DirSQL) -> Vec<String> {
    let mut out: Vec<String> = db
        .query("SELECT name FROM t")
        .unwrap()
        .iter()
        .map(|r| match r.get("name") {
            Some(Value::Text(s)) => s.clone(),
            other => panic!("name was not text: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// A repo whose `.gitignore` ignores `build/`, which holds `old.md`, watched
/// with a `**/*.md` table; then the `.gitignore` emptied.
fn un_ignored_build() -> (TempDir, DirSQL) {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::create_dir_all(dir.path().join("build/deep")).unwrap();
    fs::write(dir.path().join("build/deep/old.md"), "x").unwrap();
    fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    let table = Table::new("t", "CREATE TABLE t (name TEXT)", "**/*.md", |path| {
        let name = path.rsplit(['/', '\\']).next().unwrap().to_string();
        vec![HashMap::from([("name".into(), Value::Text(name))])]
    });
    let db = DirSQL::new(dir.path(), vec![table]).unwrap();
    assert!(names(&db).is_empty());
    thread::sleep(Duration::from_millis(250));
    fs::write(dir.path().join(".gitignore"), "").unwrap();
    (dir, db)
}

fn poll_until(db: &DirSQL, want: &[&str]) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && names(db) != want {
        db.poll_events(Duration::from_millis(200)).unwrap();
    }
    names(db)
}

// FSEvents and ReadDirectoryChangesW watch the whole tree with one watch, so
// there is no newly watched directory to walk.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn a_gitignore_edit_indexes_the_files_of_a_directory_it_un_ignores() {
    let (_dir, db) = un_ignored_build();

    assert_eq!(poll_until(&db, &["old.md"]), ["old.md"]);
}

#[test]
fn a_file_written_in_a_directory_a_gitignore_edit_un_ignores_is_indexed() {
    let (dir, db) = un_ignored_build();
    thread::sleep(Duration::from_millis(250));
    db.poll_events(Duration::from_millis(200)).unwrap();

    fs::write(dir.path().join("build/deep/new.md"), "x").unwrap();

    assert!(
        poll_until(&db, &["new.md", "old.md"]).contains(&"new.md".to_string()),
        "got: {:?}",
        names(&db)
    );
}
