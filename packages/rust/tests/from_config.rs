use dirsql::{DirSQL, Value};
use std::fs;
use tempfile::TempDir;

/// Config-defined tables produce one row per matched file. Every row's
/// columns come from filesystem facts: the stat virtuals (`path`, `basename`,
/// `dir`, `ext`, `size`, `mtime`, `ctime`). Content interpretation is
/// intentionally out of scope.

#[test]
fn from_config_produces_one_row_per_matched_file() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, basename TEXT)"
glob = "data/*.csv"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\",\"basename\":\"%s\"}" "$sep" "$rel" "${p##*/}"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "anything").unwrap();
    fs::write(root.path().join("data").join("b.csv"), "anything").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
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
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "**/*.csv"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\"}" "$sep" "$rel"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("data")).unwrap();
    fs::create_dir_all(root.path().join("ignored")).unwrap();
    fs::write(root.path().join("data").join("a.csv"), "x").unwrap();
    fs::write(root.path().join("ignored").join("b.csv"), "x").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("data/a.csv".into()));
}

// A `{name}` glob placeholder whose name is ALSO a declared DDL column is a
// load-time error: captures no longer populate columns, so the column would
// read NULL forever. The error must name the placeholder and the fix.
#[test]
fn from_config_capture_column_collision_errors() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "comments"
ddl = "CREATE TABLE comments (thread_id TEXT, basename TEXT)"
glob = "_comments/{thread_id}/*.txt"
on-file = "cat"
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("_comments").join("abc123")).unwrap();
    fs::write(
        root.path()
            .join("_comments")
            .join("abc123")
            .join("first.txt"),
        "hello",
    )
    .unwrap();

    let err = match DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
    {
        Ok(_) => {
            panic!("a {{thread_id}} placeholder colliding with the thread_id column must error")
        }
        Err(e) => e,
    };

    let msg = err.to_string();
    assert!(
        msg.contains("thread_id"),
        "error must name the colliding placeholder/column, got: {msg}"
    );
    assert!(
        msg.contains("collides"),
        "error must explain the collision, got: {msg}"
    );
}

// A `{name}` placeholder whose name is NOT a declared column keeps working
// silently: matching is unchanged (it behaves like `*`), and the file's
// filesystem columns still populate. Proves matching semantics are preserved.
#[test]
fn from_config_capture_placeholder_without_column_still_matches() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "comments"
ddl = "CREATE TABLE comments (path TEXT, basename TEXT)"
glob = "_comments/{thread_id}/*.txt"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\",\"basename\":\"%s\"}" "$sep" "$rel" "${p##*/}"; sep=","; done; printf "]"' sh {root}'''
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

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db
        .query("SELECT basename FROM comments ORDER BY basename")
        .unwrap();

    assert_eq!(rows.len(), 2, "{{thread_id}} still matches like `*`");
    assert_eq!(rows[0]["basename"], Value::Text("first.txt".into()));
    assert_eq!(rows[1]["basename"], Value::Text("second.txt".into()));
    assert!(
        !rows[0].contains_key("thread_id"),
        "the placeholder no longer produces a column value"
    );
}

#[test]
fn from_config_exposes_stat_virtuals() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, basename TEXT, dir TEXT, ext TEXT, size INTEGER, mtime INTEGER)"
glob = "docs/*.md"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for f; do p=$(printf %s "$f" | tr "\\\\" /); rel=${p#"$r"/}; base=${p##*/}; case "$rel" in */*) dir=${rel%/*};; *) dir="";; esac; ext=${base##*.}; [ "$ext" = "$base" ] && ext=""; size=$(wc -c < "$f" | tr -d " "); mtime=$(stat -c %Y "$f"); printf "%s{\"path\":\"%s\",\"basename\":\"%s\",\"dir\":\"%s\",\"ext\":\"%s\",\"size\":%s,\"mtime\":%s}" "$sep" "$rel" "$base" "$dir" "$ext" "$size" "$mtime"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    fs::create_dir_all(root.path().join("docs")).unwrap();
    let body = "# title\nhello world\n";
    fs::write(root.path().join("docs").join("readme.md"), body).unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db
        .query("SELECT path, basename, dir, ext, size, mtime FROM files")
        .unwrap();

    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["path"], Value::Text("docs/readme.md".into()));
    assert_eq!(r["basename"], Value::Text("readme.md".into()));
    assert_eq!(r["dir"], Value::Text("docs".into()));
    assert_eq!(r["ext"], Value::Text("md".into()));
    assert_eq!(
        r["size"],
        Value::Integer(i64::try_from(body.len()).unwrap())
    );
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
name = "minimal"
ddl = "CREATE TABLE minimal (path TEXT)"
glob = "*.txt"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\",\"basename\":\"%s\"}" "$sep" "$rel" "${p##*/}"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    fs::write(root.path().join("a.txt"), "x").unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT path FROM minimal").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("a.txt".into()));
}

#[test]
fn from_config_missing_config_file_returns_error() {
    let root = TempDir::new().unwrap();
    let result = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build();
    assert!(result.is_err());
}

// A `[[table]]` with no `on-file` hook emits no columns of its own, so every
// row would be all-NULL. That is never useful, so it is a load error pointing
// at the path-table replacement instead of silently producing NULL rows.
#[test]
fn from_config_hookless_table_errors() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, size INTEGER)"
glob = "**/*.md"
"#,
    )
    .unwrap();

    let err = match DirSQL::from_config_path(root.path().join(".dirsql.toml")) {
        Ok(_) => panic!("a hook-less [[table]] must fail to load"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("on-file"),
        "error must name the missing on-file hook, got: {msg}"
    );
    assert!(
        msg.contains("FROM './**'"),
        "error must point at the path-table replacement, got: {msg}"
    );
}

#[test]
fn from_config_with_no_matching_files_yields_empty_table() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "empty_t"
ddl = "CREATE TABLE empty_t (path TEXT)"
glob = "nothing_here/*.txt"
on-file = "cat"
"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT path FROM empty_t").unwrap();
    assert!(rows.is_empty());
}

// Persistence is no longer config-driven — `persist`/`persist_path` are gone
// from the TOML schema (moved to the `--persist [PATH]` CLI flag). A config
// that still carries them fails to load with an "unknown field" error.
#[test]
fn from_config_rejects_removed_persist_keys() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.csv"), "anything").unwrap();

    let cfg_path = root.path().join(".dirsql.toml");
    fs::write(
        &cfg_path,
        r#"
[dirsql]
persist = true

[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
on-file = "cat"
"#,
    )
    .unwrap();

    let err = match DirSQL::from_config_path(&cfg_path) {
        Ok(_) => panic!("expected a load error for the removed `persist` key"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("persist"),
        "expected an unknown-field error naming `persist`, got {err}"
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
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.csv"
strict = true
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\"}" "$sep" "$rel"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    let rows = db.query("SELECT path FROM files").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("a.csv".into()));
}

#[tokio::test]
async fn async_from_config_works() {
    let root = TempDir::new().unwrap();

    fs::write(
        root.path().join(".dirsql.toml"),
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT, basename TEXT)"
glob = "*.csv"
on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\",\"basename\":\"%s\"}" "$sep" "$rel" "${p##*/}"; sep=","; done; printf "]"' sh {root}'''
"#,
    )
    .unwrap();

    fs::write(root.path().join("data.csv"), "anything").unwrap();

    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build_async()
        .unwrap();
    db.ready().await.unwrap();
    let rows = db.query("SELECT path, basename FROM files").await.unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["path"], Value::Text("data.csv".into()));
    assert_eq!(rows[0]["basename"], Value::Text("data.csv".into()));
}

const PATH_HOOK: &str = r#"on-file = '''sh -c 'r=$(printf %s "$1" | tr "\\\\" /); shift; printf "["; sep=""; for p; do p=$(printf %s "$p" | tr "\\\\" /); rel=${p#"$r"/}; printf "%s{\"path\":\"%s\"}" "$sep" "$rel"; sep=","; done; printf "]"' sh {root}'''"#;

/// root.json at depth 0, folder/a.json at depth 1, folder/sub/b.json at depth 2.
fn nested_json_fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("folder").join("sub")).unwrap();
    fs::write(root.path().join("root.json"), "{}").unwrap();
    fs::write(root.path().join("folder").join("a.json"), "{}").unwrap();
    fs::write(root.path().join("folder").join("sub").join("b.json"), "{}").unwrap();
    root
}

fn paths(root: &TempDir, config: &str) -> Vec<String> {
    fs::write(root.path().join(".dirsql.toml"), config).unwrap();
    let db = DirSQL::builder()
        .root(root.path())
        .config(root.path().join(".dirsql.toml"))
        .build()
        .unwrap();
    db.query("SELECT path FROM files ORDER BY path")
        .unwrap()
        .into_iter()
        .map(|r| match &r["path"] {
            Value::Text(s) => s.clone(),
            other => panic!("path must be text, got {other:?}"),
        })
        .collect()
}

#[test]
fn from_config_a_star_glob_ingests_depth_zero_only() {
    let root = nested_json_fixture();
    let config = format!(
        r#"
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "*.json"
{PATH_HOOK}
"#
    );
    assert_eq!(paths(&root, &config), ["root.json"]);
}

#[test]
fn from_config_a_directory_slash_star_ignore_hides_only_that_directory_s_files() {
    let root = nested_json_fixture();
    let config = format!(
        r#"
[dirsql]
ignore = ["folder/*"]

[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "**/*.json"
{PATH_HOOK}
"#
    );
    assert_eq!(paths(&root, &config), ["folder/sub/b.json", "root.json"]);
}
