//! Integration tests for a quoted-identifier DDL.
//!
//! A table's name is declared, but its `ddl` is bring-your-own -- hand-written
//! or emitted by any ORM / schema tool. A **quoted identifier** (the canonical
//! form emitted by Drizzle / SQLAlchemy / Diesel / sea-query) carries SQL
//! *delimiters*, not part of the name, so SQLite records the bare identifier
//! and the declared `name` matches it.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

/// The `comments` fixture from `sdk.rs::comments_table`, differing only in
/// the **quoted** table identifier -- anything that breaks is the delimiter
/// handling, not the surrounding plumbing.
fn quoted_comments_table() -> Table {
    Table::new(
        "comments",
        r#"CREATE TABLE "comments" (id TEXT, body TEXT, author TEXT)"#,
        "comments/**/index.txt",
        |path| {
            let content = std::fs::read_to_string(path).unwrap();
            let id = std::path::Path::new(path)
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("unknown")
                .to_string();

            content
                .lines()
                .map(|line| {
                    let mut parts = line.split('|');
                    let body = parts.next().unwrap_or("").to_string();
                    let author = parts.next().unwrap_or("").to_string();
                    HashMap::from([
                        ("id".into(), Value::Text(id.clone())),
                        ("body".into(), Value::Text(body)),
                        ("author".into(), Value::Text(author)),
                    ])
                })
                .collect::<Vec<Row>>()
        },
    )
}

#[test]
fn quoted_identifier_ddl_registers_and_is_queryable() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "first comment|alice\nsecond comment|bob\n",
    )
    .unwrap();

    let db = DirSQL::new(root.path(), vec![quoted_comments_table()])
        .expect("quoted-identifier DDL should register; SQLite resolves the name to `comments`");
    let rows = db.query("SELECT * FROM comments").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
    assert_eq!(rows[0]["author"], Value::Text("alice".into()));
    assert_eq!(rows[1]["author"], Value::Text("bob".into()));
}
