use dirsql::{DirSQL, RawFileEvent, Row, Table, Value};
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

// The split-phase wait/apply API is used by async bindings (TypeScript) that
// cannot safely invoke the `extract` callback off the host thread. Verify the
// two halves round-trip to the same result as the combined `poll_events`.
#[test]
fn it_splits_wait_and_apply_for_async_bindings() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.start_watching().unwrap();

    // Empty wait returns no events and apply on empty returns no row events.
    let empty = db.wait_file_events(Duration::from_millis(50)).unwrap();
    assert!(empty.is_empty());
    assert!(db.apply_file_events(Vec::new()).is_empty());

    // Write a file, then drain raw FileEvents without running extract.
    fs::write(root.path().join("new.txt"), "hello").unwrap();
    let mut raw = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while raw.is_empty() && std::time::Instant::now() < deadline {
        raw.extend(db.wait_file_events(Duration::from_millis(250)).unwrap());
    }
    assert!(!raw.is_empty(), "expected at least one raw file event");
    assert!(
        raw.iter()
            .any(|e| matches!(e, RawFileEvent::Created(_) | RawFileEvent::Modified(_)))
    );

    // Apply runs extract and mutates the DB. Inserts land in the index.
    let row_events = db.apply_file_events(raw);
    assert!(!row_events.is_empty());

    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(rows.iter().any(|r| matches!(
        r.get("name"),
        Some(Value::Text(name)) if name == "hello"
    )));
}

// The split-phase prepare/finish build API is used by async bindings
// (TypeScript) that cannot safely invoke the `extract` callback off the host
// thread. `prepare_build` walks the directory and reads file contents on the
// worker thread; `finish_build` runs `extract` + DB inserts on the thread
// where the callback is safe. Verify both halves together produce the same
// result as the combined `DirSQL::new` constructor.
#[test]
fn it_splits_scan_and_build_for_async_bindings() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.txt"), "alpha").unwrap();
    fs::write(root.path().join("b.txt"), "beta").unwrap();

    // Counter proves `prepare_build` does NOT invoke `extract` — only
    // `finish_build` should call it, once per scanned file.
    let extract_calls = Arc::new(AtomicUsize::new(0));
    let counter = extract_calls.clone();
    let table = Table::new(
        "CREATE TABLE items (name TEXT)",
        "**/*.txt",
        move |_path, content| {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![HashMap::from([(
                "name".into(),
                Value::Text(content.trim().to_string()),
            )])]
        },
    );

    let prepared =
        DirSQL::prepare_build(root.path().to_path_buf(), vec![table], Vec::new()).unwrap();
    assert_eq!(
        extract_calls.load(Ordering::SeqCst),
        0,
        "prepare_build must not call extract"
    );

    let db = DirSQL::finish_build(prepared).unwrap();
    assert_eq!(
        extract_calls.load(Ordering::SeqCst),
        2,
        "finish_build should call extract once per scanned file"
    );

    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("alpha".into()));
    assert_eq!(rows[1]["name"], Value::Text("beta".into()));
}

// ---------------------------------------------------------------------------
// Builder API
// ---------------------------------------------------------------------------

#[test]
fn builder_root_and_table_match_new() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.txt"), "alpha").unwrap();
    fs::write(root.path().join("b.txt"), "beta").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .table(items_table())
        .build()
        .unwrap();

    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn builder_ignore_filters_files() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("skip")).unwrap();
    fs::write(root.path().join("a.txt"), "alpha").unwrap();
    fs::write(root.path().join("skip").join("b.txt"), "beta").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .table(items_table())
        .ignore(["skip/**"])
        .build()
        .unwrap();

    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("alpha".into()));
}

#[test]
fn builder_config_loads_tables_and_root() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.json"), r#"{"name":"one"}"#).unwrap();
    fs::write(root.path().join("b.json"), r#"{"name":"two"}"#).unwrap();

    let cfg_path = root.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
"#,
    )
    .unwrap();

    let db = DirSQL::builder().config(&cfg_path).build().unwrap();
    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn builder_config_root_resolves_relative_to_config_parent() {
    let root = TempDir::new().unwrap();
    let data_dir = root.path().join("data");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(data_dir.join("a.json"), r#"{"name":"one"}"#).unwrap();

    let cfg_path = root.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[dirsql]
root = "data"

[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
"#,
    )
    .unwrap();

    let db = DirSQL::builder().config(&cfg_path).build().unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
}

#[test]
fn builder_explicit_root_overrides_config_root() {
    // Config root points at an empty dir, but explicit .root() wins.
    let temp = TempDir::new().unwrap();
    let empty_dir = temp.path().join("empty");
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&empty_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(data_dir.join("x.json"), r#"{"name":"present"}"#).unwrap();

    let cfg_path = temp.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[dirsql]
root = "empty"

[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(&data_dir)
        .config(&cfg_path)
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("present".into()));
}

#[test]
fn builder_without_root_or_config_errors() {
    let result = DirSQL::builder().table(items_table()).build();
    let err = match result {
        Ok(_) => panic!("expected error when no root is provided"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("root"),
        "expected root-missing error, got: {msg}"
    );
}

#[test]
fn builder_appends_programmatic_tables_to_config_tables() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("notes")).unwrap();
    fs::write(root.path().join("notes").join("a.txt"), "hello").unwrap();
    fs::write(root.path().join("a.json"), r#"{"name":"from_config"}"#).unwrap();

    let cfg_path = root.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.json"
"#,
    )
    .unwrap();

    let notes_table = Table::new(
        "CREATE TABLE notes (body TEXT)",
        "notes/*.txt",
        |_path, content| {
            vec![HashMap::from([(
                "body".into(),
                Value::Text(content.trim().to_string()),
            )])]
        },
    );

    let db = DirSQL::builder()
        .root(root.path())
        .table(notes_table)
        .config(&cfg_path)
        .build()
        .unwrap();

    let items = db.query("SELECT name FROM items").unwrap();
    assert_eq!(items.len(), 1);
    let notes = db.query("SELECT body FROM notes").unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["body"], Value::Text("hello".into()));
}
