# Getting Started

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

:::

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
                ddl="CREATE TABLE posts (title TEXT, author TEXT)",
                glob="posts/*.json",
                extract=lambda path, content: [json.loads(content)],
            ),
            Table(
                ddl="CREATE TABLE authors (id TEXT, name TEXT)",
                glob="authors/*.json",
                extract=lambda path, content: [json.loads(content)],
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
use dirsql::{DirSQL, Table};

let db = DirSQL::new(
    "./my-blog",
    vec![
        Table::new(
            "CREATE TABLE posts (title TEXT, author TEXT)",
            "posts/*.json",
            |_path, content| vec![serde_json::from_str(content).unwrap()],
        ),
        Table::new(
            "CREATE TABLE authors (id TEXT, name TEXT)",
            "authors/*.json",
            |_path, content| vec![serde_json::from_str(content).unwrap()],
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
import { DirSQL, type TableDef } from 'dirsql';

const tables: TableDef[] = [
  {
    ddl: 'CREATE TABLE posts (title TEXT, author TEXT)',
    glob: 'posts/*.json',
    extract: (_path, content) => [JSON.parse(content)],
  },
  {
    ddl: 'CREATE TABLE authors (id TEXT, name TEXT)',
    glob: 'authors/*.json',
    extract: (_path, content) => [JSON.parse(content)],
  },
];

const db = new DirSQL('./my-blog', tables);

const posts = await db.query('SELECT * FROM posts');

const results = await db.query(`
  SELECT posts.title, authors.name
  FROM posts JOIN authors ON posts.author = authors.id
`);
```

:::

## What happens at startup

1. `dirsql` walks the directory tree
2. Files matching each table's glob pattern are read
3. The `extract` function converts file content into rows
4. Rows are inserted into an in-memory SQLite database
5. SQL queries run against that database

The filesystem is always the source of truth. The database is rebuilt from files at startup.

## Next steps

- [Defining Tables](./guide/tables.md) -- DDL, globs, and extract functions in detail
- [Querying](./guide/querying.md) -- SQL queries and return format
- [File Watching](./guide/watching.md) -- real-time change events
- [Async API](./guide/async.md) -- async ready(), query(), and watch()
