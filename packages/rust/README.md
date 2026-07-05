# `dirsql` (Rust SDK)

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface -- the filesystem is always the source of truth.

[Documentation](https://thekevinscott.github.io/dirsql/?lang=rust)

Also available as [`dirsql` on PyPI](https://pypi.org/project/dirsql/) and [`dirsql` on npm](https://www.npmjs.com/package/dirsql).

## Installation

```bash
cargo add dirsql
```

## Quick start

`DirSQL::new` scans the directory synchronously and returns a ready instance. Each table is a `(ddl, glob, extract)` triple: the DDL defines the SQLite schema, the glob selects files (relative to the root), and `extract` turns a matched file into rows (`Vec<HashMap<String, Value>>`). `dirsql` does not read file contents -- the callback reads `path` itself; return an empty `Vec` to skip a file.

```rust
use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;

// Convert a JSON object string into a dirsql row. Reused by the examples below.
fn row_from_json(raw: &str) -> HashMap<String, Value> {
    let v: serde_json::Value = serde_json::from_str(raw).unwrap();
    let serde_json::Value::Object(obj) = v else { return HashMap::new() };
    obj.into_iter()
        .map(|(k, val)| {
            let v = match val {
                serde_json::Value::String(s) => Value::Text(s),
                serde_json::Value::Number(n) => n
                    .as_i64()
                    .map(Value::Integer)
                    .unwrap_or_else(|| Value::Real(n.as_f64().unwrap_or(0.0))),
                serde_json::Value::Bool(b) => Value::Integer(b as i64),
                serde_json::Value::Null => Value::Null,
                other => Value::Text(other.to_string()),
            };
            (k, v)
        })
        .collect()
}

let db = DirSQL::new(
    "./my-blog",
    vec![Table::new(
        "CREATE TABLE posts (title TEXT, author TEXT)",
        "posts/*.json",
        |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
    )],
)?;

let posts = db.query("SELECT * FROM posts WHERE author = 'alice'")?;
```

## Multiple tables and joins

```rust
let db = DirSQL::new(
    "./my-blog",
    vec![
        Table::new(
            "CREATE TABLE posts (title TEXT, author_id TEXT)",
            "posts/*.json",
            |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
        ),
        Table::new(
            "CREATE TABLE authors (id TEXT, name TEXT)",
            "authors/*.json",
            |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
        ),
    ],
)?;

let results = db.query(
    "SELECT posts.title, authors.name \
     FROM posts JOIN authors ON posts.author_id = authors.id",
)?;
```

## Ignoring files

Use `DirSQL::with_ignore` to skip files during scanning and watching:

```rust
let db = DirSQL::with_ignore(
    "./my-blog",
    vec![/* tables */],
    vec!["**/drafts/**", "**/.git/**"],
)?;
```

## Watching for changes

`db.watch()` returns a stream of row-level change events as files change on disk. `.next()` comes from `StreamExt` in the `futures` crate (`cargo add futures`), driven inside an async runtime such as tokio:

```rust
use dirsql::RowEvent;
use futures::StreamExt;

let mut stream = db.watch()?;
while let Some(event) = stream.next().await {
    match event {
        RowEvent::Insert { table, row, file_path } => {
            println!("insert on {table} ({file_path}): {row:?}")
        }
        RowEvent::Update { table, old_row, new_row, file_path } => {
            println!("update on {table} ({file_path}): {old_row:?} -> {new_row:?}")
        }
        RowEvent::Delete { table, row, file_path } => {
            println!("delete on {table} ({file_path}): {row:?}")
        }
        RowEvent::Error { table, file_path, error } => {
            println!("error on {table:?} {file_path:?}: {error}")
        }
    }
}
```

## CLI

```bash
cargo install dirsql --features cli
dirsql
```

Running `dirsql` starts an HTTP server bound to `localhost:7117` that exposes the SDK over HTTP: `POST /query` for SQL and `GET /events` for a Server-Sent Events change stream. Override with `--host`, `--port`, `--config`. See the [CLI reference](https://thekevinscott.github.io/dirsql/reference/cli).

The `cli` feature is **opt-in** -- `cargo add dirsql` pulls no CLI dependencies. `cargo install dirsql` without `--features cli` silently installs nothing (`required-features` skips the bin target with no warning); always include the flag, or use `npx dirsql` / `uvx dirsql` for prebuilt binaries.

### Feature flags

| Feature | Default | Description |
|---|---|---|
| `cli` | no | Enables the `dirsql` binary and its dependencies. |

## License

MIT
