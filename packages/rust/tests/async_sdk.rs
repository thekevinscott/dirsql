use dirsql_sdk::{AsyncDirSQL, Row, Table, Value};
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

#[tokio::test]
async fn it_constructs_without_blocking() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "hello|alice\n",
    )
    .unwrap();

    let db = AsyncDirSQL::new(root.path(), vec![comments_table()]).unwrap();
    // Should not panic -- construction is immediate, scan runs in background
    assert!(db.ready().await.is_ok());
}

#[tokio::test]
async fn it_indexes_files_after_ready() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "first comment|alice\nsecond comment|bob\n",
    )
    .unwrap();

    let db = AsyncDirSQL::new(root.path(), vec![comments_table()]).unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT * FROM comments").await.unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn it_allows_multiple_ready_calls() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "a comment|alice\n",
    )
    .unwrap();

    let db = AsyncDirSQL::new(root.path(), vec![comments_table()]).unwrap();
    db.ready().await.unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT * FROM comments").await.unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn it_queries_asynchronously() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("comments").join("abc")).unwrap();
    fs::write(
        root.path().join("comments").join("abc").join("index.txt"),
        "first comment|alice\nsecond comment|bob\n",
    )
    .unwrap();

    let db = AsyncDirSQL::new(root.path(), vec![comments_table()]).unwrap();
    db.ready().await.unwrap();
    let rows = db
        .query("SELECT author FROM comments WHERE body = 'first comment'")
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["author"], Value::Text("alice".into()));
}

#[tokio::test]
async fn it_raises_on_invalid_sql() {
    let root = TempDir::new().unwrap();
    let db = AsyncDirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.ready().await.unwrap();
    let result = db.query("NOT VALID SQL").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn it_supports_ignore_patterns() {
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

    let db =
        AsyncDirSQL::with_ignore(root.path(), vec![comments_table()], vec!["**/def/**"]).unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT DISTINCT id FROM comments").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
}

#[tokio::test]
async fn it_streams_watch_events() {
    let root = TempDir::new().unwrap();
    let db = AsyncDirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.ready().await.unwrap();

    let mut stream = db.watch().unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;
    fs::write(root.path().join("new_item.txt"), "apple").unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timeout waiting for watch event")
        .expect("stream ended");

    match event {
        dirsql_sdk::RowEvent::Insert { table, row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
