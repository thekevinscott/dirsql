use dirsql::{DirSQL, Value};
use std::fs;
use tempfile::TempDir;

/// Helper: create a temp dir with a `.dirsql.toml` and data files, then run from_config.
fn setup_csv_config() -> (TempDir, DirSQL) {
    let root = TempDir::new().unwrap();

    // Write config
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[dirsql]
ignore = ["ignored/**"]

[[table]]
ddl = "CREATE TABLE items (name TEXT, price REAL)"
glob = "data/*.csv"
"#,
    )
    .unwrap();

    // Write data file
    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(
        root.path().join("data").join("products.csv"),
        "name,price\napple,1.50\nbanana,0.75\n",
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    (root, db)
}

#[test]
fn from_config_indexes_csv_files() {
    let (_root, db) = setup_csv_config();
    let rows = db
        .query("SELECT name, price FROM items ORDER BY name")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("apple".into()));
    assert_eq!(rows[1]["name"], Value::Text("banana".into()));
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
ddl = "CREATE TABLE items (name TEXT)"
glob = "**/*.csv"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("ignored")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "name\nvisible\n").unwrap();
    fs::write(root.path().join("ignored").join("b.csv"), "name\nhidden\n").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT name FROM items").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], Value::Text("visible".into()));
}

#[test]
fn from_config_with_json_and_each() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE products (name TEXT, price REAL)"
glob = "catalog/*.json"
each = "items"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("catalog")).unwrap();
    fs::write(
        root.path().join("catalog").join("store.json"),
        r#"{"items": [{"name": "widget", "price": 9.99}, {"name": "gadget", "price": 19.99}]}"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT name, price FROM products ORDER BY name")
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("gadget".into()));
    assert_eq!(rows[1]["name"], Value::Text("widget".into()));
}

#[test]
fn from_config_with_path_captures() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT)"
glob = "_comments/{thread_id}/index.jsonl"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("_comments").join("abc123")).unwrap();
    fs::write(
        root.path()
            .join("_comments")
            .join("abc123")
            .join("index.jsonl"),
        r#"{"body": "hello world"}
{"body": "goodbye world"}
"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db
        .query("SELECT thread_id, body FROM comments ORDER BY body")
        .unwrap();

    assert_eq!(rows.len(), 2);
    // Both rows should have thread_id = "abc123" from the path capture
    assert_eq!(rows[0]["thread_id"], Value::Text("abc123".into()));
    assert_eq!(rows[1]["thread_id"], Value::Text("abc123".into()));
}

#[test]
fn from_config_with_column_mapping() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE authors (display_name TEXT)"
glob = "authors/*.json"

[table.columns]
display_name = "metadata.author.name"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("authors")).unwrap();
    fs::write(
        root.path().join("authors").join("alice.json"),
        r#"{"metadata": {"author": {"name": "Alice Smith"}}}"#,
    )
    .unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT display_name FROM authors").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["display_name"], Value::Text("Alice Smith".into()));
}

#[test]
fn from_config_with_explicit_format() {
    let root = TempDir::new().unwrap();

    // Use format = "csv" on a .txt extension file
    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE data (x TEXT, y TEXT)"
glob = "*.txt"
format = "csv"
"#,
    )
    .unwrap();

    fs::write(root.path().join("data.txt"), "x,y\nfoo,bar\n").unwrap();

    let db = DirSQL::from_config(root.path()).unwrap();
    let rows = db.query("SELECT x, y FROM data").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["x"], Value::Text("foo".into()));
    assert_eq!(rows[0]["y"], Value::Text("bar".into()));
}

#[test]
fn from_config_missing_config_file_returns_error() {
    let root = TempDir::new().unwrap();
    let result = DirSQL::from_config(root.path());
    assert!(result.is_err());
}

#[test]
fn from_config_no_format_and_unknown_extension_returns_error() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE data (x TEXT)"
glob = "*.dat"
"#,
    )
    .unwrap();

    // The table has no format and .dat can't be inferred -- should error
    let result = DirSQL::from_config(root.path());
    assert!(result.is_err());
}

#[tokio::test]
async fn async_from_config_works() {
    use dirsql::AsyncDirSQL;

    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.csv"
"#,
    )
    .unwrap();

    fs::write(root.path().join("data.csv"), "name\nhello\nworld\n").unwrap();

    let db = AsyncDirSQL::from_config(root.path()).unwrap();
    db.ready().await.unwrap();
    let rows = db
        .query("SELECT name FROM items ORDER BY name")
        .await
        .unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], Value::Text("hello".into()));
    assert_eq!(rows[1]["name"], Value::Text("world".into()));
}
