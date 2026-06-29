//! Integration tests for robust table-name resolution (issue #204).
//!
//! dirsql keeps `ddl` as the schema input (bring-your-own DDL, hand-written
//! or emitted by any ORM / schema tool). The only thing dirsql needs from the
//! DDL is the table *name*. The hand-rolled `parse_table_name()` scanner gets
//! that wrong for perfectly valid DDL shapes -- most notably a **quoted
//! identifier**, the canonical form emitted by Drizzle / SQLAlchemy / Diesel /
//! sea-query, where it returns the name *with the surrounding quotes* and the
//! downstream identifier validator then rejects the table outright.
//!
//! #204 resolves the name via SQLite itself (execute the DDL, read the name
//! back from `sqlite_master`). These tests assert the user-visible outcome:
//! a quoted-identifier DDL registers and the table is queryable, with the
//! name resolved to the bare `comments`.
//!
//! RED today: `DirSQL::new` errors because `parse_table_name` yields
//! `"comments"` (quotes included), which `validate_identifier` rejects.

use dirsql::{DirSQL, Row, Table, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

/// The exact `comments` fixture from `sdk.rs::comments_table`, with the only
/// difference being the **quoted** table identifier in the DDL. Isolating the
/// quoting keeps the test honest: anything that breaks is the name resolution,
/// not the surrounding plumbing.
fn quoted_comments_table() -> Table {
    Table::new(
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

    // SQLite resolves the quoted DDL to the bare table name `comments`, so the
    // table must register and accept the bare name in a query.
    let db = DirSQL::new(root.path(), vec![quoted_comments_table()])
        .expect("quoted-identifier DDL should register; SQLite resolves the name to `comments`");
    let rows = db.query("SELECT * FROM comments").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
    assert_eq!(rows[0]["author"], Value::Text("alice".into()));
    assert_eq!(rows[1]["author"], Value::Text("bob".into()));
}
