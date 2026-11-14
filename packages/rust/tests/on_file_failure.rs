//! A per-file hook that fails, or returns a row the table rejects, fails the
//! build with an error naming the file, the table and the original error.

use std::fs;

use dirsql::{DirSQL, DirSqlError, Row, Table, Value};
use tempfile::TempDir;

fn tree(files: &[&str]) -> TempDir {
    let root = TempDir::new().unwrap();
    for name in files {
        fs::write(root.path().join(name), "x").unwrap();
    }
    root
}

fn row(pairs: &[(&str, Value)]) -> Row {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn build_err(root: &TempDir, tables: Vec<Table>) -> DirSqlError {
    DirSQL::new(root.path(), tables)
        .err()
        .expect("the build must fail")
}

#[test]
fn a_not_null_violation_fails_the_build() {
    let root = tree(&["a.txt"]);
    let err = build_err(
        &root,
        vec![Table::new(
            "notes",
            "CREATE TABLE notes (n INTEGER NOT NULL)",
            "*.txt",
            |_| vec![row(&[("n", Value::Null)])],
        )],
    );
    assert!(
        matches!(&err, DirSqlError::OnFile { table, path, message }
            if table == "notes" && path == "a.txt" && message.contains("NOT NULL")),
        "{err:?}"
    );
}

#[test]
fn a_unique_violation_fails_the_build() {
    let root = tree(&["a.txt", "b.txt"]);
    let err = build_err(
        &root,
        vec![Table::new(
            "notes",
            "CREATE TABLE notes (id TEXT UNIQUE)",
            "*.txt",
            |_| vec![row(&[("id", Value::Text("same".into()))])],
        )],
    );
    assert!(
        matches!(&err, DirSqlError::OnFile { table, message, .. }
            if table == "notes" && message.contains("UNIQUE")),
        "{err:?}"
    );
}

#[test]
fn a_parent_hook_failure_is_reported_ahead_of_the_orphaned_child() {
    let root = tree(&["b.txt"]);
    let err = build_err(
        &root,
        vec![
            Table::try_new(
                "parent",
                "CREATE TABLE parent (id TEXT PRIMARY KEY)",
                "*.txt",
                |_| Err("rejected-xyzzy".into()),
            ),
            Table::new(
                "child",
                "CREATE TABLE child (parent TEXT REFERENCES parent(id))",
                "*.txt",
                |_| vec![row(&[("parent", Value::Text("b".into()))])],
            ),
        ],
    );
    assert_eq!(
        err.to_string(),
        "table `parent`, file `b.txt`: rejected-xyzzy"
    );
}

#[cfg(target_os = "linux")]
fn counted_notes(root: &TempDir) -> DirSQL {
    DirSQL::new(
        root.path(),
        vec![Table::try_new(
            "notes",
            "CREATE TABLE notes (n INTEGER)",
            "*.txt",
            |path| -> Result<Vec<Row>, Box<dyn std::error::Error + Send + Sync>> {
                let text = fs::read_to_string(path)?;
                let n: i64 = text.trim().parse()?;
                Ok(vec![row(&[("n", Value::Integer(n))])])
            },
        )],
    )
    .unwrap()
}

#[cfg(target_os = "linux")]
fn notes_n(db: &DirSQL) -> Result<Vec<Value>, DirSqlError> {
    Ok(db
        .query("SELECT n FROM notes")?
        .into_iter()
        .map(|row| row["n"].clone())
        .collect())
}

/// Wait for a query to answer as `done` holds: the watcher's events reach
/// the index after the write returns.
#[cfg(target_os = "linux")]
fn await_query(db: &DirSQL, done: fn(&Result<Vec<Value>, DirSqlError>) -> bool) {
    for _ in 0..1000 {
        if done(&notes_n(db)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the query never answered as expected: {:?}", notes_n(db));
}

#[test]
#[cfg(target_os = "linux")]
fn every_query_raises_a_changed_files_failure_until_the_file_is_fixed() {
    let root = tree(&[]);
    fs::write(root.path().join("a.txt"), "1").unwrap();
    let db = counted_notes(&root);
    assert_eq!(notes_n(&db).unwrap(), [Value::Integer(1)]);

    fs::write(root.path().join("a.txt"), "half-writ").unwrap();
    await_query(&db, Result::is_err);
    for query in 0..3 {
        let err = notes_n(&db).expect_err("a query while the file fails must raise");
        assert!(
            matches!(&err, DirSqlError::OnFile { table, path, message }
                if table == "notes" && path == "a.txt" && message.contains("invalid digit")),
            "query {query}: {err:?}"
        );
    }

    fs::write(root.path().join("a.txt"), "22").unwrap();
    await_query(&db, Result::is_ok);
    assert_eq!(notes_n(&db).unwrap(), [Value::Integer(22)]);
    assert_eq!(notes_n(&db).unwrap(), [Value::Integer(22)]);
}
