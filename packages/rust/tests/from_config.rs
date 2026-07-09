use dirsql::{DirSQL, Value};
use std::fs;
use tempfile::TempDir;

/// Config-defined tables produce one row per matched file. Every row's
/// columns come from filesystem facts: glob path captures and stat virtuals
/// (`path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`).
/// Content interpretation is intentionally out of scope.

#[test]
fn from_config_produces_one_row_per_matched_file() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (path TEXT, basename TEXT)"
glob = "data/*.csv"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "anything").unwrap();
    fs::write(root.path().join("data").join("b.csv"), "anything").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT path, basename FROM files ORDER BY path")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["path"], Value::Text("data/a.csv".into()));
    assert_eq!(rows[0]["basename"], Value::Text("a.csv".into()));
    assert_eq!(rows[1]["path"], Value::Text("data/b.csv".into()));
    assert_eq!(rows[1]["basename"], Value::Text("b.csv".into()));
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
ddl = "CREATE TABLE files (path TEXT)"
glob = "**/*.csv"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("ignored")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "x").unwrap();
    fs::write(root.path().join("ignored").join("b.csv"), "x").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("data/a.csv".into()));
}

#[test]
fn from_config_with_path_captures_promotes_them_to_columns() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"
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
        .query("SELECT thread_id, basename FROM comments ORDER BY thread_id")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["thread_id"], Value::Text("abc123".into()));
    assert_eq!(rows[0]["basename"], Value::Text("first.txt".into()));
    assert_eq!(rows[1]["thread_id"], Value::Text("def456".into()));
    assert_eq!(rows[1]["basename"], Value::Text("second.txt".into()));
}

#[test]
fn from_config_exposes_stat_virtuals() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER)"
glob = "docs/*.md"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("docs")).unwrap();
    let body = "# title\nhello world\n";
    fs::write(root.path().join("docs").join("readme.md"), body).unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT path, basename, dir, ext, size, mtime FROM files")
        .unwrap();

    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["path"], Value::Text("docs/readme.md".into()));
    assert_eq!(r["basename"], Value::Text("readme.md".into()));
    assert_eq!(r["dir"], Value::Text("docs".into()));
    assert_eq!(r["ext"], Value::Text("md".into()));
    assert_eq!(r["size"], Value::Integer(body.len() as i64));
    // mtime is a unix timestamp; confirm it's a positive integer.
    assert!(
        matches!(&r["mtime"], Value::Integer(n) if *n > 0),
        "expected a positive Integer mtime, got {:?}",
        r["mtime"]
    );
}

#[test]
fn from_config_undeclared_stat_columns_are_silently_dropped() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE minimal (path TEXT)"
glob = "*.txt"
"#,
    )
    .unwrap();

    fs::write(root.path().join("a.txt"), "x").unwrap();
    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM minimal").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("a.txt".into()));
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
ddl = "CREATE TABLE empty_t (path TEXT)"
glob = "nothing_here/*.txt"
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM empty_t").unwrap();
    assert!(rows.is_empty());
}

// A relative `persist_path` resolves against the config's parent directory.
#[test]
fn from_config_persist_true_with_relative_persist_path() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "anything").unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
persist = true
persist_path = "cache/db.sqlite"

[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        root.path().join("cache").join("db.sqlite").exists(),
        "expected the cache db at the resolved relative persist_path",
    );
}

// An absolute `persist_path` is used verbatim.
#[test]
fn from_config_persist_true_with_absolute_persist_path() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "anything").unwrap();

    let cache_dir = TempDir::new().unwrap();
    let abs_cache = cache_dir.path().join("nested").join("abs-cache.db");

    fs::write(
        root.path().join(".dirsql.toml"),
        format!(
            r#"
[dirsql]
persist = true
persist_path = "{}"

[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
"#,
            abs_cache.display()
        ),
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        abs_cache.exists(),
        "expected the cache db at the absolute persist_path",
    );
}

#[test]
fn from_config_strict_table_builds() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "anything").unwrap();

    // Declare only `path` (always available) so strict normalization, which
    // requires an exact column match, succeeds: the synthesized empty row is
    // filled with `path` and no undeclared virtuals leak in.
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
strict = true
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("a.csv".into()));
}

#[tokio::test]
async fn async_from_config_works() {
    use dirsql::AsyncDirSQL;

    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE files (path TEXT, basename TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();

    fs::write(root.path().join("data.csv"), "anything").unwrap();

    let db = AsyncDirSQL::from_config(root.path()).unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT path, basename FROM files").await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("data.csv".into()));
    assert_eq!(rows[0]["basename"], Value::Text("data.csv".into()));
}
