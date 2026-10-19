//! Integration tests for declared foreign keys. SQLite enforces them, and
//! tables fill in no fixed order, so a constraint is checked once per unit of
//! work: the build, or one batch of watch events.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use dirsql::watcher::FileEvent;
use dirsql::{DirSQL, Row, RowEvent, Table, Value};
use tempfile::TempDir;

const PARENT_DDL: &str = "CREATE TABLE parent (id INTEGER PRIMARY KEY, name TEXT)";
const CHILD_DDL: &str = "CREATE TABLE child (name TEXT, parent_id INTEGER REFERENCES parent(id))";

fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn contents(path: &str) -> String {
    fs::read_to_string(path).unwrap().trim().to_string()
}

/// One `parent` row per file: the file's stem is the id, its contents the name.
fn parent_table(glob: &str) -> Table {
    Table::new("parent", PARENT_DDL, glob, |path| {
        vec![HashMap::from([
            ("id".into(), Value::Integer(stem(path).parse().unwrap())),
            ("name".into(), Value::Text(contents(path))),
        ])] as Vec<Row>
    })
}

/// One `child` row per file: the file's stem is the name, its contents the
/// id of the parent it references.
fn child_table(glob: &str) -> Table {
    Table::new("child", CHILD_DDL, glob, |path| {
        vec![HashMap::from([
            ("name".into(), Value::Text(stem(path))),
            (
                "parent_id".into(),
                Value::Integer(contents(path).parse().unwrap()),
            ),
        ])] as Vec<Row>
    })
}

fn names(db: &DirSQL, sql: &str) -> Vec<String> {
    db.query(sql)
        .unwrap()
        .into_iter()
        .map(|row| match row.get("name") {
            Some(Value::Text(name)) => name.clone(),
            other => panic!("expected a name, got {other:?}"),
        })
        .collect()
}

fn errors(events: &[RowEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            RowEvent::Error { error, .. } => Some(error.clone()),
            _ => None,
        })
        .collect()
}

/// A tree of `parents/<id>.txt` files, built, then `children/<name>.txt`
/// files added through one watch batch each, so the build's fill order plays
/// no part.
fn family(parents: &[(&str, &str)], children: &[(&str, &str)]) -> (TempDir, PathBuf, DirSQL) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    fs::create_dir(root.join("parents")).unwrap();
    fs::create_dir(root.join("children")).unwrap();
    for (id, name) in parents {
        fs::write(root.join("parents").join(format!("{id}.txt")), name).unwrap();
    }
    let db = DirSQL::new(
        &root,
        vec![child_table("children/*.txt"), parent_table("parents/*.txt")],
    )
    .unwrap();
    for (name, parent_id) in children {
        let child = root.join("children").join(format!("{name}.txt"));
        fs::write(&child, parent_id).unwrap();
        let events = db.apply_file_events(vec![FileEvent::Created(child)]);
        assert_eq!(errors(&events), Vec::<String>::new());
    }
    (dir, root, db)
}

#[test]
fn a_child_filled_before_its_parent_builds() {
    // One file feeds both tables, so the declared order is the fill order:
    // the child row lands before the parent row it references.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("1.txt"), "1").unwrap();

    let db = DirSQL::new(
        dir.path(),
        vec![child_table("*.txt"), parent_table("*.txt")],
    )
    .unwrap();

    assert_eq!(
        names(
            &db,
            "SELECT p.name FROM child c JOIN parent p ON p.id = c.parent_id"
        ),
        vec!["1"]
    );
}

#[test]
fn an_orphan_fails_the_build_naming_the_table_and_the_row() {
    let dir = TempDir::new().unwrap();
    fs::create_dir(dir.path().join("parents")).unwrap();
    fs::create_dir(dir.path().join("children")).unwrap();
    fs::write(dir.path().join("parents").join("1.txt"), "alpha").unwrap();
    fs::write(dir.path().join("children").join("lost.txt"), "2").unwrap();

    let err = DirSQL::new(
        dir.path(),
        vec![child_table("children/*.txt"), parent_table("parents/*.txt")],
    )
    .err()
    .expect("an orphaned child row must fail the build")
    .to_string();

    for needle in ["child", "parent", "children/lost.txt", "parent_id = 2"] {
        assert!(err.contains(needle), "{needle:?} missing from: {err}");
    }
}

#[test]
fn editing_a_referenced_parent_in_watch_updates_it() {
    let (_dir, root, db) = family(&[("1", "alpha")], &[("kid", "1")]);
    let parent = root.join("parents").join("1.txt");
    fs::write(&parent, "renamed").unwrap();

    let events = db.apply_file_events(vec![FileEvent::Modified(parent)]);

    assert_eq!(errors(&events), Vec::<String>::new());
    assert_eq!(names(&db, "SELECT name FROM parent"), vec!["renamed"]);
}

#[test]
fn a_parent_and_its_child_arriving_in_one_batch_both_land() {
    let (_dir, root, db) = family(&[], &[]);
    let child = root.join("children").join("kid.txt");
    let parent = root.join("parents").join("7.txt");
    fs::write(&child, "7").unwrap();
    fs::write(&parent, "seven").unwrap();

    let events = db.apply_file_events(vec![FileEvent::Created(child), FileEvent::Created(parent)]);

    assert_eq!(errors(&events), Vec::<String>::new());
    assert_eq!(
        names(
            &db,
            "SELECT p.name FROM child c JOIN parent p ON p.id = c.parent_id"
        ),
        vec!["seven"]
    );
}

#[test]
fn orphaning_a_child_in_watch_reports_it_and_keeps_the_previous_state() {
    let (_dir, root, db) = family(&[("1", "alpha"), ("2", "beta")], &[("kid", "1")]);
    let parent = root.join("parents").join("1.txt");
    fs::remove_file(&parent).unwrap();

    let events = db.apply_file_events(vec![FileEvent::Deleted(parent)]);

    let reported = errors(&events);
    assert_eq!(reported.len(), 1, "expected one error, got {events:?}");
    for needle in ["child", "parent", "children/kid.txt", "parent_id = 1"] {
        assert!(
            reported[0].contains(needle),
            "{needle:?} missing from: {}",
            reported[0]
        );
    }
    assert_eq!(
        events.len(),
        1,
        "a rolled-back batch reports no row changes: {events:?}"
    );
    assert_eq!(
        names(&db, "SELECT name FROM parent ORDER BY id"),
        vec!["alpha", "beta"]
    );

    let other = root.join("parents").join("2.txt");
    fs::write(&other, "beta renamed").unwrap();
    let events = db.apply_file_events(vec![FileEvent::Modified(other)]);
    assert_eq!(errors(&events), Vec::<String>::new());
    assert_eq!(
        names(&db, "SELECT name FROM parent ORDER BY id"),
        vec!["alpha", "beta renamed"]
    );
}
