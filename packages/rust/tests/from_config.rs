use dirsql::{DirSQL, Value};
use std::fs;
use tempfile::TempDir;

/// Config-defined tables produce one row per matched file. Every row's
/// columns come from filesystem facts: glob path captures and stat virtuals
/// (`_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`).
/// Content interpretation is intentionally out of scope.

#[test]
fn from_config_produces_one_row_per_matched_file() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (_path TEXT, _basename TEXT)"
glob = "data/*.csv"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "anything").unwrap();
    fs::write(root.path().join("data").join("b.csv"), "anything").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT _path, _basename FROM files ORDER BY _path")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["_path"], Value::Text("data/a.csv".into()));
    assert_eq!(rows[0]["_basename"], Value::Text("a.csv".into()));
    assert_eq!(rows[1]["_path"], Value::Text("data/b.csv".into()));
    assert_eq!(rows[1]["_basename"], Value::Text("b.csv".into()));
}

#[test]
fn from_config_honors_ignore_patterns() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
ignore = ["ignored/**"]

[[table]]
ddl = "CREATE TABLE files (_path TEXT)"
glob = "**/*.csv"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("ignored")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "x").unwrap();
    fs::write(root.path().join("ignored").join("b.csv"), "x").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT _path FROM files").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["_path"], Value::Text("data/a.csv".into()));
}

#[test]
fn from_config_with_path_captures_promotes_them_to_columns() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, _basename TEXT)"
glob = "_comments/{thread_id}/*.txt"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("_comments").join("abc123")).unwrap();
    fs::create_dir_all(root.path().join("_comments").join("def456")).unwrap();
    fs::write(
        root.path()
            .join("_comments")
            .join("abc123")
            .join("first.txt"),
        "hello",
    )
    .unwrap();
    fs::write(
        root.path()
            .join("_comments")
            .join("def456")
            .join("second.txt"),
        "world",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT thread_id, _basename FROM comments ORDER BY thread_id")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["thread_id"], Value::Text("abc123".into()));
    assert_eq!(rows[0]["_basename"], Value::Text("first.txt".into()));
    assert_eq!(rows[1]["thread_id"], Value::Text("def456".into()));
    assert_eq!(rows[1]["_basename"], Value::Text("second.txt".into()));
}

#[test]
fn from_config_exposes_stat_virtuals() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER)"
glob = "docs/*.md"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("docs")).unwrap();
    let body = "# title\nhello world\n";
    fs::write(root.path().join("docs").join("readme.md"), body).unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT _path, _basename, _dir, _ext, _size, _mtime FROM files")
        .unwrap();

    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["_path"], Value::Text("docs/readme.md".into()));
    assert_eq!(r["_basename"], Value::Text("readme.md".into()));
    assert_eq!(r["_dir"], Value::Text("docs".into()));
    assert_eq!(r["_ext"], Value::Text("md".into()));
    assert_eq!(r["_size"], Value::Integer(body.len() as i64));
    // _mtime is set to a unix timestamp; just confirm it's a positive integer.
    match &r["_mtime"] {
        Value::Integer(n) => assert!(*n > 0, "expected positive _mtime, got {n}"),
        other => panic!("expected Integer for _mtime, got {:?}", other),
    }
}

#[test]
fn from_config_undeclared_stat_columns_are_silently_dropped() {
    // The DDL declares only _path; _size/_mtime are not selected, but the
    // injection layer doesn't fail when they're not in the table schema.
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE minimal (_path TEXT)"
glob = "*.txt"
"#,
    )
    .unwrap();

    fs::write(root.path().join("a.txt"), "x").unwrap();
    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT _path FROM minimal").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["_path"], Value::Text("a.txt".into()));
}

#[test]
fn from_config_missing_config_file_returns_error() {
    let root = TempDir::new().unwrap();
    let result = DirSQL::from_config(root.path());
    assert!(result.is_err());
}

#[test]
fn from_config_with_no_matching_files_yields_empty_table() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE empty_t (_path TEXT)"
glob = "nothing_here/*.txt"
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT _path FROM empty_t").unwrap();
    assert!(rows.is_empty());
}

#[tokio::test]
async fn async_from_config_works() {
    use dirsql::AsyncDirSQL;

    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (_path TEXT, _basename TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();

    fs::write(root.path().join("data.csv"), "anything").unwrap();

    let db = AsyncDirSQL::from_config(root.path()).unwrap();
    db.ready().await.unwrap();
    let rows = db
        .query("SELECT _path, _basename FROM files")
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["_path"], Value::Text("data.csv".into()));
    assert_eq!(rows[0]["_basename"], Value::Text("data.csv".into()));
}
