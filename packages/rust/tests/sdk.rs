use dirsql_sdk::{DirSQL, Row, Table, Value};
use futures_executor::block_on;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

fn comments_table() -> Table {
    Table::new(
        "CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
        "comments/**/index.txt",
        |path, content| {
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

fn items_table() -> Table {
    Table::new(
        "CREATE TABLE items (name TEXT)",
        "**/*.txt",
        |_, content| {
            vec![HashMap::from([(
                "name".into(),
                Value::Text(content.trim().to_string()),
            )])]
        },
    )
}

#[test]
fn it_indexes_and_queries_rows() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "first comment|alice\nsecond comment|bob\n",
    )
    .unwrap();

    let db = DirSQL::new(root.path(), vec![comments_table()]).unwrap();
    let rows = db.query("SELECT * FROM comments").unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
    assert_eq!(rows[0]["author"], Value::Text("alice".into()));
}

#[test]
fn it_honors_ignore_patterns() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::create_dir_all(root.path().join("comments").join("def")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "first comment|alice\n",
    )
    .unwrap();
    fs::write(
        root.path().join("comments").join("def").join("index.txt"),
        "ignored comment|bob\n",
    )
    .unwrap();

    let db = DirSQL::with_ignore(root.path(), vec![comments_table()], vec!["**/def/**"]).unwrap();
    let rows = db.query("SELECT DISTINCT id FROM comments").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
}

#[test]
fn it_supports_multiple_tables_and_joins() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("posts")).unwrap();
    fs::create_dir_all(root.path().join("authors")).unwrap();
    fs::write(root.path().join("posts").join("hello.txt"), "Hello World|1").unwrap();
    fs::write(root.path().join("authors").join("alice.txt"), "1|Alice").unwrap();

    let posts = Table::new(
        "CREATE TABLE posts (title TEXT, author_id TEXT)",
        "posts/*.txt",
        |_, content| {
            content
                .lines()
                .map(|line| {
                    let mut parts = line.split('|');
                    HashMap::from([
                        (
                            "title".into(),
                            Value::Text(parts.next().unwrap_or("").to_string()),
                        ),
                        (
                            "author_id".into(),
                            Value::Text(parts.next().unwrap_or("").to_string()),
                        ),
                    ])
                })
                .collect()
        },
    );
    let authors = Table::new(
        "CREATE TABLE authors (id TEXT, name TEXT)",
        "authors/*.txt",
        |_, content| {
            content
                .lines()
                .map(|line| {
                    let mut parts = line.split('|');
                    HashMap::from([
                        (
                            "id".into(),
                            Value::Text(parts.next().unwrap_or("").to_string()),
                        ),
                        (
                            "name".into(),
                            Value::Text(parts.next().unwrap_or("").to_string()),
                        ),
                    ])
                })
                .collect()
        },
    );

    let db = DirSQL::new(root.path(), vec![posts, authors]).unwrap();
    let rows = db
        .query("SELECT posts.title, authors.name FROM posts JOIN authors ON posts.author_id = authors.id")
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("Hello World".into()));
    assert_eq!(rows[0]["name"], Value::Text("Alice".into()));
}

#[test]
fn it_streams_watch_events() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("new_item.txt"), "apple").unwrap();

    let event = block_on(stream.next()).expect("watch event");
    match event {
        dirsql_sdk::RowEvent::Insert { table, row } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
