//! A statement that names `content` has the column read for every row it
//! selects before SQLite delivers the first of them, rather than one file at
//! a time as the cursor is stepped. Real temp trees, real SQLite, a raw
//! connection so the rows can be stepped one at a time with the tree changed
//! in between: a file deleted after the first row still has its content in
//! the rows that follow, which a per-row read could not supply.

use std::fs;
use std::path::{Path, PathBuf};

use dirsql::vtab::{StatementScope, load_module};
use rusqlite::{Connection, Rows};
use tempfile::TempDir;

const PAPERS: usize = 8;

/// `PAPERS` directories, each holding a `title.md` and an `abstract.md`;
/// the paths of the titles, in scan order.
fn papers(root: &Path) -> Vec<PathBuf> {
    (0..PAPERS)
        .map(|i| {
            let dir = root.join(format!("p{i:02}"));
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("abstract.md"), format!("abstract {i}")).unwrap();
            let title = dir.join("title.md");
            fs::write(&title, format!("title {i}")).unwrap();
            title
        })
        .collect()
}

fn titles() -> Vec<Option<String>> {
    (0..PAPERS).map(|i| Some(format!("title {i}"))).collect()
}

/// A connection with path-tables `a` over the abstracts and `t` over the
/// titles under `root`.
fn open(root: &Path) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    load_module(&conn, StatementScope::new()).unwrap();
    for (name, glob) in [("a", "*/abstract.md"), ("t", "*/title.md")] {
        conn.execute_batch(&format!(
            "CREATE VIRTUAL TABLE {name} USING dirsql_path('{}', '{glob}', '', 'gitignore')",
            root.display()
        ))
        .unwrap();
    }
    conn
}

fn first(rows: &mut Rows<'_>) -> Option<String> {
    rows.next().unwrap().expect("a first row").get(0).unwrap()
}

fn rest(rows: &mut Rows<'_>) -> Vec<Option<String>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next().unwrap() {
        out.push(row.get(0).unwrap());
    }
    out
}

fn delete(paths: &[PathBuf]) {
    for path in paths {
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn content_is_read_for_every_row_before_the_first_is_delivered() {
    let root = TempDir::new().unwrap();
    let paths = papers(root.path());
    let conn = open(root.path());
    let mut stmt = conn.prepare("SELECT content FROM t").unwrap();
    let mut rows = stmt.query([]).unwrap();

    assert_eq!(first(&mut rows).as_deref(), Some("title 0"));
    delete(&paths);

    assert_eq!(
        rest(&mut rows),
        titles()[1..],
        "the titles were deleted after the first row; a row read when stepped finds nothing"
    );
}

#[test]
fn a_where_on_content_has_the_column_read_ahead_too() {
    let root = TempDir::new().unwrap();
    let paths = papers(root.path());
    let conn = open(root.path());
    let mut stmt = conn
        .prepare("SELECT basename FROM t WHERE content LIKE 'title%'")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();

    assert_eq!(first(&mut rows).as_deref(), Some("title.md"));
    delete(&paths);

    assert_eq!(
        rest(&mut rows).len(),
        PAPERS - 1,
        "every title matched before the deletion; a row read when stepped matches nothing"
    );
}

#[test]
fn a_join_has_the_content_it_names_read_ahead() {
    let root = TempDir::new().unwrap();
    let paths = papers(root.path());
    let conn = open(root.path());
    let mut stmt = conn
        .prepare("SELECT t.content FROM a JOIN t ON t.dir = a.dir")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();

    assert_eq!(first(&mut rows).as_deref(), Some("title 0"));
    delete(&paths);

    assert_eq!(
        rest(&mut rows),
        titles()[1..],
        "the titles were deleted after the first joined row; a side read as it is probed finds nothing"
    );
}
