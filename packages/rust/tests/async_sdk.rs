use dirsql::{AsyncDirSQL, Row, Table, Value};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

fn comments_table() -> Table {
    Table::new(
        "comments",
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
    Table::new(
        "items",
        "CREATE TABLE items (name TEXT)",
        "**/*.txt",
        |path| {
            let content = std::fs::read_to_string(path).unwrap();
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
async fn from_config_path_loads_config_with_explicit_root() {
    // `from_config_path` roots at the process cwd; the explicit `.root(...)`
    // on the builder points the index at the data directory (#540).
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("data.csv"), "anything").unwrap();
    let cfg_path = root.path().join("custom.toml");
    fs::write(
        &cfg_path,
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
on-file = "printf '[{}]'"
"#,
    )
    .unwrap();

    let db = dirsql::DirSQL::builder()
        .root(root.path())
        .config(&cfg_path)
        .build_async()
        .unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT path FROM files").await.unwrap();
    assert_eq!(rows.len(), 1);
}

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

/// Deadlock backstop for the parked-init handshake below. Generous: nothing
/// here is a timing assertion, it is the bound that turns a scan which never
/// reaches the extract closure into a failure instead of a wedged test binary.
const PARK_PATIENCE: Duration = Duration::from_secs(30);

/// The parked-init handshake: `parked` is raised by the extract closure on
/// arrival, `released` by the test once its assertions are done.
#[derive(Default)]
struct Gate {
    parked: bool,
    released: bool,
}

#[tokio::test]
async fn sync_backed_methods_before_ready_error() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("gate.txt"), "apple").unwrap();

    // Background init runs eagerly, so merely "not awaiting ready()" races
    // it. Instead, park init deterministically: the extract closure blocks
    // until the assertions release it, so init cannot complete during the
    // assertion window.
    let gate = std::sync::Arc::new((
        std::sync::Mutex::new(Gate::default()),
        std::sync::Condvar::new(),
    ));
    let gate_in_extract = gate.clone();
    let gated_table = Table::new(
        "items",
        "CREATE TABLE items (name TEXT)",
        "**/*.txt",
        move |_| {
            let (lock, cvar) = &*gate_in_extract;
            let mut state = lock.lock().unwrap();
            state.parked = true;
            cvar.notify_all();
            let _ = cvar
                .wait_timeout_while(state, PARK_PATIENCE, |s| !s.released)
                .unwrap();
            vec![HashMap::from([(
                "name".into(),
                Value::Text("apple".into()),
            )])]
        },
    );

    let db = AsyncDirSQL::new(root.path(), vec![gated_table]).unwrap();
    // Every sync-backed method threads `self.sync()?` first, so all surface
    // the not-ready `DirSqlError::Lock`. (`sync()`/`watch()` return non-Debug
    // Ok types, so match the error out.)
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

    // Init parks in the extract closure, which is reached only if the scan
    // matched `gate.txt`. Prove it arrived, then release it and let it finish
    // so the background thread does not outlive the TempDir.
    let (lock, cvar) = &*gate;
    let (mut state, wait) = cvar
        .wait_timeout_while(lock.lock().unwrap(), PARK_PATIENCE, |s| !s.parked)
        .unwrap();
    assert!(
        !wait.timed_out(),
        "init never reached the extract closure: the scan matched no file, so \
         this test never exercised a parked init",
    );
    state.released = true;
    cvar.notify_all();
    drop(state);
    db.ready().await.unwrap();
}

#[tokio::test]
async fn init_failure_surfaces_through_ready_and_sync() {
    let root = TempDir::new().unwrap();
    let t1 = Table::new("dup", "CREATE TABLE dup (a TEXT)", "*.a", |_| vec![]);
    let t2 = Table::new("dup", "CREATE TABLE dup (b TEXT)", "*.b", |_| vec![]);
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
