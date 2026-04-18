use dirsql::{DirSQL, Row, Table, Value};
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
        dirsql::RowEvent::Insert { table, row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn it_ignores_extra_keys_by_default() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "apple|red|150").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |_, content| {
                let mut parts = content.trim().split('|');
                let name = parts.next().unwrap_or("").to_string();
                let color = parts.next().unwrap_or("").to_string();
                vec![HashMap::from([
                    ("name".into(), Value::Text(name)),
                    ("color".into(), Value::Text(color)),
                    ("weight".into(), Value::Integer(150)),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert!(!rows[0].contains_key("color"));
    assert!(!rows[0].contains_key("weight"));
}

#[test]
fn it_fills_missing_keys_with_null() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "apple").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT, color TEXT, count INTEGER)",
            "*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert_eq!(rows[0]["color"], Value::Null);
    assert_eq!(rows[0]["count"], Value::Null);
}

#[test]
fn it_raises_on_extra_keys_in_strict_mode() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "apple|red").unwrap();

    let result = DirSQL::new(
        root.path(),
        vec![Table::strict(
            "CREATE TABLE items (name TEXT)",
            "*.txt",
            |_, content| {
                let mut parts = content.trim().split('|');
                let name = parts.next().unwrap_or("").to_string();
                let color = parts.next().unwrap_or("").to_string();
                vec![HashMap::from([
                    ("name".into(), Value::Text(name)),
                    ("color".into(), Value::Text(color)),
                ])]
            },
        )],
    );

    assert!(result.is_err());
}

#[test]
fn it_raises_on_missing_keys_in_strict_mode() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "apple").unwrap();

    let result = DirSQL::new(
        root.path(),
        vec![Table::strict(
            "CREATE TABLE items (name TEXT, color TEXT)",
            "*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    );

    assert!(result.is_err());
}

#[test]
fn it_allows_exact_match_in_strict_mode() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "apple|red").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::strict(
            "CREATE TABLE items (name TEXT, color TEXT)",
            "*.txt",
            |_, content| {
                let mut parts = content.trim().split('|');
                let name = parts.next().unwrap_or("").to_string();
                let color = parts.next().unwrap_or("").to_string();
                vec![HashMap::from([
                    ("name".into(), Value::Text(name)),
                    ("color".into(), Value::Text(color)),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert_eq!(rows[0]["color"], Value::Text("red".into()));
}

#[test]
fn it_streams_watch_delete_events() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("doomed.txt"), "doomed").unwrap();

    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();

    // Verify initial data
    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);

    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::remove_file(root.path().join("doomed.txt")).unwrap();

    let event = block_on(stream.next()).expect("watch event");
    match event {
        dirsql::RowEvent::Delete { table, row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("doomed".into()));
        }
        other => panic!("expected delete event, got: {other:?}"),
    }
}

#[test]
fn it_streams_watch_update_events() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "draft").unwrap();

    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("item.txt"), "final").unwrap();

    let event = block_on(stream.next()).expect("watch event");
    // Could be Update or Delete+Insert
    match event {
        dirsql::RowEvent::Update { table, new_row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(new_row["name"], Value::Text("final".into()));
        }
        dirsql::RowEvent::Delete { table, .. } => {
            assert_eq!(table, "items");
        }
        dirsql::RowEvent::Insert { table, row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("final".into()));
        }
        other => panic!("expected update-related event, got: {other:?}"),
    }
}

#[test]
fn it_streams_watch_error_events() {
    let root = TempDir::new().unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::try_new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_, _content| Err("intentional parse failure".into()),
        )],
    )
    .unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("bad.txt"), "data").unwrap();

    let event = block_on(stream.next()).expect("watch event");
    match event {
        dirsql::RowEvent::Error {
            table,
            error,
            file_path,
        } => {
            assert!(error.contains("intentional parse failure"));
            assert_eq!(
                table.as_deref(),
                Some("items"),
                "error event should attribute the failure to the matching table"
            );
            assert!(file_path.to_string_lossy().contains("bad.txt"));
        }
        other => panic!("expected error event, got: {other:?}"),
    }
}
