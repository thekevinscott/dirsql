use dirsql::{AsyncDirSQL, Row, Table, Value};
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

// build_async() surfaces resolve() errors (e.g. no root) via its `?`.
#[tokio::test]
async fn build_async_without_root_errors() {
    let err = match dirsql::DirSQL::builder().table(items_table()).build_async() {
        Ok(_) => panic!("expected a Config error when no root is provided"),
        Err(e) => e,
    };
    assert!(
        matches!(err, dirsql::DirSqlError::Config { .. }),
        "got: {err}"
    );
}

// AsyncDirSQL::from_config_path loads a config from an explicit path.
#[tokio::test]
async fn from_config_path_loads_config() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("data.csv"), "anything").unwrap();
    let cfg_path = root.path().join("custom.toml");
    fs::write(
        &cfg_path,
        r#"
[dirsql]
root = "."

[[table]]
ddl = "CREATE TABLE files (_path TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();

    let db = AsyncDirSQL::from_config_path(&cfg_path).unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT _path FROM files").await.unwrap();
    assert_eq!(rows.len(), 1);
}

// start_watching + poll_events forward to the inner DirSQL once ready.
#[tokio::test]
async fn start_watching_and_poll_events_forward() {
    let root = TempDir::new().unwrap();
    let db = AsyncDirSQL::new(root.path(), vec![items_table()]).unwrap();
    db.ready().await.unwrap();
    db.start_watching().unwrap();

    tokio::time::sleep(Duration::from_millis(250)).await;
    fs::write(root.path().join("apple.txt"), "apple").unwrap();

    let mut events = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while events.is_empty() && std::time::Instant::now() < deadline {
        events.extend(db.poll_events(Duration::from_millis(250)).unwrap());
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, dirsql::RowEvent::Insert { .. }))
    );
}

// Calling sync()-backed methods before ready() errors with the not-ready arm.
// Each forwarding method threads `self.sync()?` first, so all of them surface
// the not-ready error before doing any work.
#[tokio::test]
async fn sync_backed_methods_before_ready_error() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("gate.txt"), "apple").unwrap();

    // Background init runs eagerly, so merely "not awaiting ready()" races it
    // (#333). Instead, park init deterministically: the extract closure blocks
    // on a barrier the test releases only after the assertions, so the initial
    // scan -- and therefore init -- cannot complete during the assertion window.
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate_in_extract = gate.clone();
    let gated_table = Table::new("CREATE TABLE items (name TEXT)", "**/*.txt", move |_| {
        gate_in_extract.wait();
        vec![HashMap::from([(
            "name".into(),
            Value::Text("apple".into()),
        )])]
    });

    let db = AsyncDirSQL::new(root.path(), vec![gated_table]).unwrap();
    // Init is parked inside the scan, so sync() takes the None arm, which
    // yields the not-ready `DirSqlError::Lock`. Every sync-backed method
    // threads `self.sync()?` first, so all of them surface that same variant.
    // (`sync()`/`watch()` return non-Debug Ok types, so match the error out.)
    let sync_err = match db.sync() {
        Ok(_) => panic!("sync() before ready() must error"),
        Err(e) => e,
    };
    assert!(
        matches!(sync_err, dirsql::DirSqlError::Lock(_)),
        "got: {sync_err}"
    );

    let start_err = db.start_watching().unwrap_err();
    assert!(
        matches!(start_err, dirsql::DirSqlError::Lock(_)),
        "got: {start_err}"
    );

    let poll_err = db.poll_events(Duration::from_millis(10)).unwrap_err();
    assert!(
        matches!(poll_err, dirsql::DirSqlError::Lock(_)),
        "got: {poll_err}"
    );

    let watch_err = match db.watch() {
        Ok(_) => panic!("watch() before ready() must error"),
        Err(e) => e,
    };
    assert!(
        matches!(watch_err, dirsql::DirSqlError::Lock(_)),
        "got: {watch_err}"
    );

    // Release init and let it finish so the background thread does not
    // outlive the TempDir.
    gate.wait();
    db.ready().await.unwrap();
}

// A config that fails to build (duplicate table) makes init fail; ready(),
// sync(), and query() then surface the init-failed error arms.
#[tokio::test]
async fn init_failure_surfaces_through_ready_and_sync() {
    let root = TempDir::new().unwrap();
    let t1 = Table::new("CREATE TABLE dup (a TEXT)", "*.a", |_| vec![]);
    let t2 = Table::new("CREATE TABLE dup (b TEXT)", "*.b", |_| vec![]);
    let db = AsyncDirSQL::new(root.path(), vec![t1, t2]).unwrap();

    let ready = db.ready().await;
    assert!(ready.is_err(), "ready() must report the init failure");

    let synced = db.sync();
    assert!(synced.is_err(), "sync() must report the init failure");

    let queried = db.query("SELECT 1").await;
    assert!(queried.is_err(), "query() must report the init failure");
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
        dirsql::RowEvent::Insert { table, row, .. } => {
            assert_eq!(table, "items");
            assert_eq!(row["name"], Value::Text("apple".into()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}
