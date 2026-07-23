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

fn items_table() -> Table {
    Table::new("CREATE TABLE items (name TEXT)", "**/*.txt", |path| {
        let content = std::fs::read_to_string(path).unwrap();
        vec![HashMap::from([(
            "name".into(),
            Value::Text(content.trim().to_string()),
        )])]
    })
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
        |path| {
            let content = std::fs::read_to_string(path).unwrap();
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
        |path| {
            let content = std::fs::read_to_string(path).unwrap();
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
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
            |path| {
                let content = std::fs::read_to_string(path).unwrap();
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
fn it_round_trips_blob_values_through_the_sdk() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("marker.json"), "{}").unwrap();

    let payload: Vec<u8> = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
    let payload_for_closure = payload.clone();

    let db = DirSQL::new(
        root.path(),
        vec![Table::new(
            "CREATE TABLE blobs (name TEXT, data BLOB)",
            "*.json",
            move |_path| {
                vec![HashMap::from([
                    ("name".into(), Value::Text("bin".into())),
                    ("data".into(), Value::Blob(payload_for_closure.clone())),
                ])]
            },
        )],
    )
    .unwrap();

    let rows = db.query("SELECT name, data FROM blobs").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("bin".into()));
    assert_eq!(rows[0]["data"], Value::Blob(payload));
}

#[test]
fn it_streams_watch_delete_events() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("doomed.txt"), "doomed").unwrap();

    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();

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
            |_| Err("intentional parse failure".into()),
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

// The split-phase wait/apply API exists for async bindings (TypeScript) that
// cannot safely invoke the `extract` callback off the host thread.
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

    // Apply runs extract and mutates the DB.
    let row_events = db.apply_file_events(raw);
    assert!(!row_events.is_empty());

    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(rows.iter().any(|r| matches!(
        r.get("name"),
        Some(Value::Text(name)) if name == "hello"
    )));
}

// The split-phase prepare/finish build API exists for async bindings
// (TypeScript) that cannot safely invoke the `extract` callback off the host
// thread: `prepare_build` walks the directory on the worker thread;
// `finish_build` runs `extract` + DB inserts where the callback is safe.
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
    let table = Table::new("CREATE TABLE items (name TEXT)", "**/*.txt", move |path| {
        let content = std::fs::read_to_string(path).unwrap();
        counter.fetch_add(1, Ordering::SeqCst);
        vec![HashMap::from([(
            "name".into(),
            Value::Text(content.trim().to_string()),
        )])]
    });

    let prepared = DirSQL::builder()
        .root(root.path().to_path_buf())
        .tables(vec![table])
        .prepare()
        .unwrap();
    assert_eq!(
        extract_calls.load(Ordering::SeqCst),
        0,
        "prepare must not call extract"
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
fn builder_config_loads_tables_with_explicit_root() {
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

    let db = DirSQL::builder()
        .root(root.path())
        .config(&cfg_path)
        .build()
        .unwrap();
    let rows = db.query("SELECT name FROM items ORDER BY name").unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn builder_explicit_root_wins_over_config_directory() {
    // With `root` gone from config (#540), the index root is the explicit
    // `.root(...)`, never the config file's own directory. The config's parent
    // holds a decoy; only the explicit root's file is indexed. The `basename`
    // column is filesystem-derived so the test doesn't depend on content
    // parsing.
    let temp = TempDir::new().unwrap();
    let cfg_dir = temp.path().join("cfgdir");
    let data_dir = temp.path().join("data");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(cfg_dir.join("decoy.json"), "anything").unwrap();
    fs::write(data_dir.join("present.json"), "anything").unwrap();

    let cfg_path = cfg_dir.join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
ddl = "CREATE TABLE items (basename TEXT)"
glob = "*.json"
on-file = '''sh -c 'printf "[{\"basename\":\"%s\"}]" "${1##*/}"' sh {path}'''
"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(&data_dir)
        .config(&cfg_path)
        .build()
        .unwrap();
    let rows = db.query("SELECT basename FROM items").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["basename"], Value::Text("present.json".into()));
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

    let notes_table = Table::new("CREATE TABLE notes (body TEXT)", "notes/*.txt", |path| {
        let content = std::fs::read_to_string(path).unwrap();
        vec![HashMap::from([(
            "body".into(),
            Value::Text(content.trim().to_string()),
        )])]
    });

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

#[test]
fn poll_events_returns_row_events_for_new_file() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.start_watching().unwrap();

    std::thread::sleep(Duration::from_millis(250));
    fs::write(root.path().join("apple.txt"), "apple").unwrap();

    let mut events = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while events.is_empty() && std::time::Instant::now() < deadline {
        events.extend(db.poll_events(Duration::from_millis(250)).unwrap());
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, dirsql::RowEvent::Insert { .. }))
    );
    let rows = db.query("SELECT name FROM items").unwrap();
    assert!(rows.iter().any(|r| matches!(
        r.get("name"),
        Some(Value::Text(name)) if name == "apple"
    )));
}

// poll_events and watch() drain the same underlying watcher, so they are
// mutually exclusive.
#[test]
fn poll_events_after_watch_errors() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    let _stream = db.watch().unwrap();
    let result = db.poll_events(Duration::from_millis(50));
    assert!(result.is_err());
}

#[test]
fn watch_after_poll_events_errors() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.poll_events(Duration::from_millis(50)).unwrap();
    let result = db.watch();
    assert!(result.is_err());
}

#[test]
fn watch_twice_errors_with_already_started() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    let _stream = db.watch().unwrap();
    let result = db.watch();
    assert!(matches!(
        result,
        Err(dirsql::DirSqlError::WatchAlreadyStarted)
    ));
}

#[test]
fn wait_file_events_after_watch_errors() {
    let root = TempDir::new().unwrap();
    let db = DirSQL::new(root.path(), vec![items_table()]).unwrap();
    let _stream = db.watch().unwrap();
    let result = db.wait_file_events(Duration::from_millis(50));
    assert!(result.is_err());
}

#[test]
fn unparseable_ddl_errors() {
    let root = TempDir::new().unwrap();
    let table = Table::new("THIS IS NOT A CREATE TABLE", "*.txt", |_| vec![]);
    let result = DirSQL::new(root.path(), vec![table]);
    assert!(matches!(result, Err(dirsql::DirSqlError::Ddl(_))));
}

#[test]
fn invalid_glob_errors() {
    let root = TempDir::new().unwrap();
    let table = Table::new("CREATE TABLE t (x TEXT)", "a[b", |_| vec![]);
    let result = DirSQL::new(root.path(), vec![table]);
    assert!(matches!(result, Err(dirsql::DirSqlError::Matcher { .. })));
}

// A `{name}` placeholder that is not a declared column is a pure match
// wildcard: it produces no column value and no error.
#[test]
fn undeclared_capture_is_dropped() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("logs")).unwrap();
    fs::write(root.path().join("logs").join("a.txt"), "x").unwrap();
    let table = Table::new(
        "CREATE TABLE entries (path TEXT)",
        "logs/{kind}.txt",
        |_| vec![Row::new()],
    );
    let db = DirSQL::new(root.path(), vec![table]).unwrap();
    let rows = db.query("SELECT * FROM entries").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].contains_key("kind"));
    assert!(rows[0].contains_key("path"));
}

#[test]
fn duplicate_table_name_errors() {
    let root = TempDir::new().unwrap();
    let t1 = Table::new("CREATE TABLE dup (a TEXT)", "*.a", |_| vec![]);
    let t2 = Table::new("CREATE TABLE dup (b TEXT)", "*.b", |_| vec![]);
    let result = DirSQL::new(root.path(), vec![t1, t2]);
    assert!(matches!(result, Err(dirsql::DirSqlError::DuplicateTable(name)) if name == "dup"));
}

#[test]
fn on_file_error_surfaces_as_on_file_error() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("boom.txt"), "data").unwrap();
    let table = Table::try_new("CREATE TABLE items (name TEXT)", "*.txt", |_| {
        Err("kaboom".into())
    });
    let err = match DirSQL::new(root.path(), vec![table]) {
        Ok(_) => panic!("expected an on-file error from the failing on-file closure"),
        Err(e) => e,
    };
    match err {
        dirsql::DirSqlError::OnFile { message, path } => {
            assert!(message.contains("kaboom"));
            assert!(path.contains("boom.txt"));
        }
        other => panic!("expected OnFile error, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Fan-out: a file matching N tables' globs populates all N tables (#580).
// ---------------------------------------------------------------------------

/// Root containing exactly one file `data/2401.00001/metadata.json`.
fn fanout_root() -> TempDir {
    let root = TempDir::new().unwrap();
    let sub = root.path().join("data").join("2401.00001");
    fs::create_dir_all(&sub).unwrap();
    fs::write(sub.join("metadata.json"), "{}").unwrap();
    root
}

fn table_returning(name: &str, glob: &str, col: &'static str, val: &'static str) -> Table {
    Table::new(
        &format!("CREATE TABLE {name} ({col} TEXT)"),
        glob,
        move |_path| vec![HashMap::from([(col.into(), Value::Text(val.into()))])],
    )
}

#[test]
fn fanout_identical_globs_populate_both_tables() {
    let root = fanout_root();
    let ta = table_returning("ta", "data/*/metadata.json", "col_a", "A");
    let tb = table_returning("tb", "data/*/metadata.json", "col_b", "B");

    let db = DirSQL::new(root.path(), vec![ta, tb]).unwrap();

    let a_rows = db.query("SELECT col_a FROM ta").unwrap();
    assert_eq!(a_rows.len(), 1, "ta populated");
    assert_eq!(a_rows[0]["col_a"], Value::Text("A".into()));

    let b_rows = db.query("SELECT col_b FROM tb").unwrap();
    assert_eq!(b_rows.len(), 1, "tb (second-declared) populated");
    assert_eq!(b_rows[0]["col_b"], Value::Text("B".into()));
}

#[test]
fn fanout_overlapping_distinct_globs_populate_both_tables() {
    let root = fanout_root();
    let ta = table_returning("ta", "data/*/metadata.json", "col_a", "A");
    let tb = table_returning("tb", "data/**/metadata.json", "col_b", "B");

    let db = DirSQL::new(root.path(), vec![ta, tb]).unwrap();

    let a_rows = db.query("SELECT col_a FROM ta").unwrap();
    assert_eq!(a_rows.len(), 1, "ta populated");
    let b_rows = db.query("SELECT col_b FROM tb").unwrap();
    assert_eq!(b_rows.len(), 1, "tb (second-declared) populated");
    assert_eq!(b_rows[0]["col_b"], Value::Text("B".into()));
}

// A programmatic table whose glob declares a `{name}` placeholder colliding
// with one of its DDL columns is rejected at construction, just like a
// config-file table: captures no longer populate columns.
#[test]
fn capture_column_collision_errors_on_construction() {
    let root = fanout_root();
    let a = Table::new(
        "CREATE TABLE a (id TEXT, col_a TEXT)",
        "data/{id}/metadata.json",
        |_path| vec![HashMap::from([("col_a".into(), Value::Text("A".into()))])],
    );

    let err = match DirSQL::new(root.path(), vec![a]) {
        Ok(_) => panic!("a {{id}} placeholder colliding with the id column must error"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("id") && msg.contains("collides"),
        "error must name the collision, got: {msg}"
    );
}

#[test]
fn binary_file_under_glob_does_not_break_build() {
    // dirsql must not eagerly read matched files as UTF-8 text: a non-UTF-8
    // file under a table's glob still produces its filesystem-facts row.
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("logo.png"),
        [0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x80, 0x90],
    )
    .unwrap();

    let table = Table::new(
        "CREATE TABLE assets (path TEXT, basename TEXT)",
        "*.png",
        |path| {
            let mut row = Row::new();
            if let Some(base) = std::path::Path::new(path).file_name() {
                row.insert(
                    "basename".into(),
                    Value::Text(base.to_string_lossy().into_owned()),
                );
            }
            vec![row]
        },
    );

    let db = DirSQL::new(root.path(), vec![table]).expect("build must not fail on a binary file");

    let rows = db.query("SELECT basename FROM assets").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["basename"], Value::Text("logo.png".into()));
}

#[test]
fn builder_with_no_config_defines_no_named_tables() {
    // A builder with no `.config()` and no programmatic tables defines no
    // named tables; path-tables serve filesystem queries instead.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("readme.md"), "hello").unwrap();

    let db = DirSQL::builder().root(root.path()).build().unwrap();
    let rows = db
        .query("SELECT basename FROM './' ORDER BY basename")
        .unwrap();
    let names: Vec<Value> = rows.iter().map(|r| r["basename"].clone()).collect();
    assert!(
        names.contains(&Value::Text("readme.md".into())),
        "no-config builder must serve path-tables, got {names:?}"
    );
}
