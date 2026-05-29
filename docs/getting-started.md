---
canonical: https://thekevinscott.github.io/dirsql/getting-started
---

# Getting Started

> Online: <https://thekevinscott.github.io/dirsql/getting-started>

## Installation

::: code-group

```bash [Python]
pip install dirsql
```

```bash [Rust]
cargo add dirsql
```

```bash [TypeScript]
pnpm add dirsql
```

```bash [CLI]
# Pick whichever install path you already have handy
npx dirsql --version
uvx dirsql --version
cargo install dirsql --features cli
```

:::

See the [CLI section](./cli/) for details on the command-line interface, and the [Rust library README](https://github.com/thekevinscott/dirsql/tree/main/packages/rust) for the library-vs-CLI feature split.

## Quick start

Suppose you have a directory of JSON files representing blog posts:

```
my-blog/
  posts/
    hello.json      # {"title": "Hello World", "author": "alice"}
    second.json     # {"title": "Second Post", "author": "bob"}
  authors/
    alice.json      # {"id": "alice", "name": "Alice"}
    bob.json        # {"id": "bob", "name": "Bob"}
```

Index and query them with `dirsql`:

::: code-group

```python [Python]
import asyncio
import json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "./my-blog",
        tables=[
            Table(
                name="posts",
                glob="posts/*.json",
                columns=[
                    {"name": "title", "type": "TEXT"},
                    {"name": "author", "type": "TEXT"},
                ],
                extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
            Table(
                name="authors",
                glob="authors/*.json",
                columns=[
                    {"name": "id", "type": "TEXT"},
                    {"name": "name", "type": "TEXT"},
                ],
                extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
        ],
    )
    await db.ready()

    # Query all posts
    posts = await db.query("SELECT * FROM posts")
    # [{"title": "Hello World", "author": "alice"}, {"title": "Second Post", "author": "bob"}]

    # Join across tables
    results = await db.query("""
        SELECT posts.title, authors.name
        FROM posts
        JOIN authors ON posts.author = authors.id
    """)
    # [{"title": "Hello World", "name": "Alice"}, {"title": "Second Post", "name": "Bob"}]

asyncio.run(main())
```

```rust [Rust]
use dirsql::{Column, ColumnType, DirSQL, Table, Value};
use std::collections::HashMap;

// Convert a JSON object string into a dirsql row.
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
    vec![
        Table::from_columns(
            "posts",
            "posts/*.json",
            vec![
                Column::new("title", ColumnType::Text),
                Column::new("author", ColumnType::Text),
            ],
            |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
        ),
        Table::from_columns(
            "authors",
            "authors/*.json",
            vec![
                Column::new("id", ColumnType::Text),
                Column::new("name", ColumnType::Text),
            ],
            |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
        ),
    ],
)?;

let posts = db.query("SELECT * FROM posts")?;

let results = db.query(
    "SELECT posts.title, authors.name \
     FROM posts JOIN authors ON posts.author = authors.id"
)?;
```

```typescript [TypeScript]
import { readFileSync } from 'node:fs';
import { DirSQL, type TableDef } from 'dirsql';

const tables: TableDef[] = [
  {
    name: 'posts',
    glob: 'posts/*.json',
    columns: [
      { name: 'title', type: 'TEXT' },
      { name: 'author', type: 'TEXT' },
    ],
    extract: (path) => [JSON.parse(readFileSync(path, 'utf8'))],
  },
  {
    name: 'authors',
    glob: 'authors/*.json',
    columns: [
      { name: 'id', type: 'TEXT' },
      { name: 'name', type: 'TEXT' },
    ],
    extract: (path) => [JSON.parse(readFileSync(path, 'utf8'))],
  },
];

const db = new DirSQL({ root: './my-blog', tables });

const posts = await db.query('SELECT * FROM posts');

const results = await db.query(`
  SELECT posts.title, authors.name
  FROM posts JOIN authors ON posts.author = authors.id
`);
```

:::

## What happens at startup

1. `dirsql` walks the directory tree
2. Files matching each table's glob pattern are identified
3. The `extract` function receives each matched file's absolute path and returns rows
4. Rows are inserted into an in-memory SQLite database
5. SQL queries run against that database

The filesystem is always the source of truth. The database is rebuilt from files at startup.

## Next steps

- [Defining Tables](./guide/tables.md) -- columns, globs, and extract functions in detail
- [Querying](./guide/querying.md) -- SQL queries and return format
- [File Watching](./guide/watching.md) -- real-time change events
- [Async API](./guide/async.md) -- async ready(), query(), and watch()
- [Using `dirsql` from the CLI](./cli/) -- `dirsql` runs an HTTP server (`POST /query`, `GET /events` SSE)
- [Collaboration with CRDTs](./guide/crdt.md) -- multi-writer document merging alongside `dirsql`
