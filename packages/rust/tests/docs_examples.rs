//! Integration tests that mirror every code example in the docs.
//!
//! Each test is named to match the doc page and section it verifies.
//! If a doc example changes and these tests break, the docs need updating (or vice versa).

use dirsql_sdk::{DirSQL, Table, Value};
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Set up the blog directory structure from getting-started.md.
fn blog_dir(root: &std::path::Path) {
    let posts = root.join("posts");
    let authors = root.join("authors");
    fs::create_dir_all(&posts).unwrap();
    fs::create_dir_all(&authors).unwrap();

    fs::write(
        posts.join("hello.json"),
        r#"{"title": "Hello World", "author": "alice"}"#,
    )
    .unwrap();
    fs::write(
        posts.join("second.json"),
        r#"{"title": "Second Post", "author": "bob"}"#,
    )
    .unwrap();
    fs::write(
        authors.join("alice.json"),
        r#"{"id": "alice", "name": "Alice"}"#,
    )
    .unwrap();
    fs::write(authors.join("bob.json"), r#"{"id": "bob", "name": "Bob"}"#).unwrap();
}

fn blog_tables() -> Vec<Table> {
    vec![
        Table::new(
            "CREATE TABLE posts (title TEXT, author TEXT)",
            "posts/*.json",
            |_path, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "title".into(),
                        Value::Text(obj["title"].as_str().unwrap().to_string()),
                    ),
                    (
                        "author".into(),
                        Value::Text(obj["author"].as_str().unwrap().to_string()),
                    ),
                ])]
            },
        ),
        Table::new(
            "CREATE TABLE authors (id TEXT, name TEXT)",
            "authors/*.json",
            |_path, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "id".into(),
                        Value::Text(obj["id"].as_str().unwrap().to_string()),
                    ),
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                ])]
            },
        ),
    ]
}

// ---------------------------------------------------------------------------
// getting-started.md
// ---------------------------------------------------------------------------

#[test]
fn it_matches_getting_started_query_all_posts() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db.query("SELECT * FROM posts").unwrap();

    assert_eq!(rows.len(), 2);
    let mut titles: Vec<String> = rows
        .iter()
        .map(|r| match &r["title"] {
            Value::Text(t) => t.clone(),
            _ => panic!("expected text"),
        })
        .collect();
    titles.sort();
    assert_eq!(titles, vec!["Hello World", "Second Post"]);
}

#[test]
fn it_matches_getting_started_join_example() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db
        .query(
            "SELECT posts.title, authors.name \
             FROM posts JOIN authors ON posts.author = authors.id",
        )
        .unwrap();

    assert_eq!(rows.len(), 2);
    let mut result: Vec<(String, String)> = rows
        .iter()
        .map(|r| {
            let title = match &r["title"] {
                Value::Text(t) => t.clone(),
                _ => panic!("expected text"),
            };
            let name = match &r["name"] {
                Value::Text(n) => n.clone(),
                _ => panic!("expected text"),
            };
            (title, name)
        })
        .collect();
    result.sort();
    assert_eq!(
        result,
        vec![
            ("Hello World".to_string(), "Alice".to_string()),
            ("Second Post".to_string(), "Bob".to_string()),
        ]
    );
}

// ---------------------------------------------------------------------------
// guide/tables.md
// ---------------------------------------------------------------------------

#[test]
fn it_matches_tables_guide_single_object_json() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("item.json"), r#"{"name": "widget", "value": 42}"#).unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT, value INTEGER)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                    (
                        "value".into(),
                        Value::Integer(obj["value"].as_i64().unwrap()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("widget".into()));
    assert_eq!(rows[0]["value"], Value::Integer(42));
}

#[test]
fn it_matches_tables_guide_multiple_rows_per_file() {
    let root = TempDir::new().unwrap();
    let comments = root.path().join("comments").join("abc");
    fs::create_dir_all(&comments).unwrap();
    fs::write(comments.join("index.txt"), "first|alice\nsecond|bob\n").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE comments (body TEXT, author TEXT)",
            "comments/**/index.txt",
            |_, content| {
                content
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        let mut parts = line.split('|');
                        HashMap::from([
                            (
                                "body".into(),
                                Value::Text(parts.next().unwrap_or("").to_string()),
                            ),
                            (
                                "author".into(),
                                Value::Text(parts.next().unwrap_or("").to_string()),
                            ),
                        ])
                    })
                    .collect()
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM comments").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_matches_tables_guide_derive_from_path() {
    let root = TempDir::new().unwrap();
    let comments = root.path().join("comments").join("abc");
    fs::create_dir_all(&comments).unwrap();
    fs::write(comments.join("index.txt"), "hello\n").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE comments (id TEXT, body TEXT)",
            "comments/**/index.txt",
            |path, content| {
                let id = std::path::Path::new(path)
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                content
                    .lines()
                    .filter(|l| !l.is_empty())
                    .map(|line| {
                        HashMap::from([
                            ("id".into(), Value::Text(id.clone())),
                            ("body".into(), Value::Text(line.to_string())),
                        ])
                    })
                    .collect()
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM comments").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
    assert_eq!(rows[0]["body"], Value::Text("hello".into()));
}

#[test]
fn it_matches_tables_guide_skip_files() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("draft.json"),
        r#"{"title": "Draft Post", "draft": true}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("published.json"),
        r#"{"title": "Published Post", "draft": false}"#,
    )
    .unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE posts (title TEXT)",
            "*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                if obj.get("draft").and_then(|d| d.as_bool()).unwrap_or(false) {
                    return vec![];
                }
                vec![HashMap::from([(
                    "title".into(),
                    Value::Text(obj["title"].as_str().unwrap().to_string()),
                )])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM posts").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("Published Post".into()));
}

#[test]
fn it_matches_tables_guide_multiple_tables() {
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
    let post_rows = db.query("SELECT * FROM posts").unwrap();
    let author_rows = db.query("SELECT * FROM authors").unwrap();
    assert_eq!(post_rows.len(), 1);
    assert_eq!(author_rows.len(), 1);
}

#[test]
fn it_matches_tables_guide_ignore_patterns() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("node_modules")).unwrap();

    fs::write(root.path().join("data").join("item.txt"), "real").unwrap();
    fs::write(root.path().join("node_modules").join("dep.txt"), "ignored").unwrap();

    let db = DirSQL::with_ignore(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
        vec!["**/node_modules/**"],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("real".into()));
}

#[test]
fn it_matches_tables_guide_typed_columns() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(
        data.join("metric.json"),
        r#"{"name": "cpu", "value": 0.85, "count": 100}"#,
    )
    .unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE metrics (name TEXT, value REAL, count INTEGER)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                    ("value".into(), Value::Real(obj["value"].as_f64().unwrap())),
                    (
                        "count".into(),
                        Value::Integer(obj["count"].as_i64().unwrap()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM metrics").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("cpu".into()));
    assert_eq!(rows[0]["count"], Value::Integer(100));
    match rows[0]["value"] {
        Value::Real(v) => assert!((v - 0.85).abs() < 1e-10),
        _ => panic!("expected Real"),
    }
}

#[test]
fn it_matches_tables_guide_constraints() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("item.json"), r#"{"id": "abc", "name": "Widget"}"#).unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "id".into(),
                        Value::Text(obj["id"].as_str().unwrap().to_string()),
                    ),
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::Text("abc".into()));
    assert_eq!(rows[0]["name"], Value::Text("Widget".into()));
}

// ---------------------------------------------------------------------------
// guide/querying.md
// ---------------------------------------------------------------------------

#[test]
fn it_matches_querying_guide_select_all() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db.query("SELECT * FROM posts").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn it_matches_querying_guide_where_filter() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db
        .query("SELECT * FROM posts WHERE author = 'alice'")
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], Value::Text("Hello World".into()));
}

#[test]
fn it_matches_querying_guide_aggregation() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db
        .query("SELECT author, COUNT(*) as n FROM posts GROUP BY author")
        .unwrap();
    assert_eq!(rows.len(), 2);

    for row in &rows {
        assert_eq!(row["n"], Value::Integer(1));
    }
}

#[test]
fn it_matches_querying_guide_return_format() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db.query("SELECT title, author FROM posts").unwrap();
    assert!(!rows.is_empty());
    for row in &rows {
        assert!(row.contains_key("title"));
        assert!(row.contains_key("author"));
    }
}

#[test]
fn it_matches_querying_guide_internal_columns_excluded() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db.query("SELECT * FROM posts LIMIT 1").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].contains_key("_dirsql_file_path"));
    assert!(!rows[0].contains_key("_dirsql_row_index"));
}

#[test]
fn it_matches_querying_guide_error_handling() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let result = db.query("NOT VALID SQL");
    assert!(result.is_err());
}

#[test]
fn it_matches_querying_guide_empty_results() {
    let root = TempDir::new().unwrap();
    blog_dir(root.path());

    let db = DirSQL::new(root.path(), blog_tables()).unwrap();
    let rows = db
        .query("SELECT * FROM posts WHERE author = 'nobody'")
        .unwrap();
    assert!(rows.is_empty());
}

// ---------------------------------------------------------------------------
// guide/watching.md - watch events (sync DirSQL)
// ---------------------------------------------------------------------------

#[test]
fn it_matches_watching_guide_insert_event() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(250));
    fs::write(root.path().join("new_item.txt"), "apple").unwrap();

    let event = futures_executor::block_on(futures_util::StreamExt::next(&mut stream))
        .expect("expected watch event");
    match event {
        dirsql_sdk::RowEvent::Insert { table, row } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
        }
        other => panic!("expected insert event, got: {other:?}"),
    }
}

#[test]
fn it_matches_watching_guide_delete_event() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("doomed.txt"), "doomed").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap();

    // Verify initial data
    let rows = db.query("SELECT * FROM items").unwrap();
    assert_eq!(rows.len(), 1);

    let mut stream = db.watch().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(250));
    fs::remove_file(root.path().join("doomed.txt")).unwrap();

    let event = futures_executor::block_on(futures_util::StreamExt::next(&mut stream))
        .expect("expected watch event");
    match event {
        dirsql_sdk::RowEvent::Delete { table, row } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("doomed".into()));
        }
        other => panic!("expected delete event, got: {other:?}"),
    }
}

#[test]
fn it_matches_watching_guide_update_event() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item.txt"), "draft").unwrap();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT)",
            "**/*.txt",
            |_, content| {
                vec![HashMap::from([(
                    "name".into(),
                    Value::Text(content.trim().to_string()),
                )])]
            },
        )],
    )
    .unwrap();

    let mut stream = db.watch().unwrap();

    std::thread::sleep(std::time::Duration::from_millis(250));
    fs::write(root.path().join("item.txt"), "final").unwrap();

    let event = futures_executor::block_on(futures_util::StreamExt::next(&mut stream))
        .expect("expected watch event");

    // Could be Update or Delete+Insert depending on implementation
    match event {
        dirsql_sdk::RowEvent::Update {
            table,
            new_row,
            old_row,
        } => {
            assert_eq!(table, "items");
            assert_eq!(new_row["name"], Value::Text("final".into()));
            assert_eq!(old_row["name"], Value::Text("draft".into()));
        }
        dirsql_sdk::RowEvent::Delete { table, .. } => {
            assert_eq!(table, "items");
            // Expect the insert to follow
        }
        dirsql_sdk::RowEvent::Insert { table, row } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("final".into()));
        }
        other => panic!("expected update-related event, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// guide/async.md
// ---------------------------------------------------------------------------

#[tokio::test]
async fn it_matches_async_guide_basic_usage() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("a.json"), r#"{"name": "low", "value": 5}"#).unwrap();
    fs::write(data.join("b.json"), r#"{"name": "high", "value": 15}"#).unwrap();

    let db = dirsql_sdk::AsyncDirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT, value INTEGER)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                    (
                        "value".into(),
                        Value::Integer(obj["value"].as_i64().unwrap()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    db.ready().await.unwrap();

    let rows = db
        .query("SELECT * FROM items WHERE value > 10")
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("high".into()));
    assert_eq!(rows[0]["value"], Value::Integer(15));
}

#[tokio::test]
async fn it_matches_async_guide_ready_idempotent() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("item.json"), r#"{"name": "test", "value": 1}"#).unwrap();

    let db = dirsql_sdk::AsyncDirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT, value INTEGER)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                    (
                        "value".into(),
                        Value::Integer(obj["value"].as_i64().unwrap()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    db.ready().await.unwrap();
    db.ready().await.unwrap();

    let rows = db.query("SELECT * FROM items").await.unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn it_matches_async_guide_count_query() {
    let root = TempDir::new().unwrap();
    let data = root.path().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(data.join("a.json"), r#"{"name": "one", "value": 1}"#).unwrap();
    fs::write(data.join("b.json"), r#"{"name": "two", "value": 2}"#).unwrap();

    let db = dirsql_sdk::AsyncDirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE items (name TEXT, value INTEGER)",
            "data/*.json",
            |_, content| {
                let v: serde_json::Value = serde_json::from_str(content).unwrap();
                let obj = v.as_object().unwrap();
                vec![HashMap::from([
                    (
                        "name".into(),
                        Value::Text(obj["name"].as_str().unwrap().to_string()),
                    ),
                    (
                        "value".into(),
                        Value::Integer(obj["value"].as_i64().unwrap()),
                    ),
                ])]
            },
        )],
    )
    .unwrap();

    db.ready().await.unwrap();
    let rows = db.query("SELECT COUNT(*) as n FROM items").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["n"], Value::Integer(2));
}
