---
layout: home
hero:
  name: dirsql
  tagline: Ephemeral SQL index over a local directory
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/thekevinscott/dirsql
---

## What is `dirsql`?

`dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. On shutdown, the database is discarded -- the filesystem remains the source of truth.

## The problem

Structured data stored as flat files (JSONL, JSON, markdown) is easy to read, write, diff, and version-control. But querying across many files is slow. "Show me all unresolved comments across 50 documents" requires opening and parsing every file.

## The solution

`dirsql` bridges this gap: files remain the source of truth, but you get SQL queries and real-time change events for free. Define tables with glob patterns and extract functions, and `dirsql` handles the rest.

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-project",
    tables=[
        Table(
            ddl="CREATE TABLE comments (id TEXT, body TEXT, resolved INTEGER)",
            glob="comments/**/*.jsonl",
            extract=lambda path, content: [
                json.loads(line) for line in content.splitlines()
            ],
        ),
    ],
)

# SQL queries over your filesystem
unresolved = db.query("SELECT * FROM comments WHERE resolved = 0")
```

```rust [Rust]
use dirsql_sdk::{DirSQL, Table};

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::new(
            "CREATE TABLE comments (id TEXT, body TEXT, resolved INTEGER)",
            "comments/**/*.jsonl",
            |path, content| {
                content.lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect()
            },
        ),
    ],
)?;

let unresolved = db.query("SELECT * FROM comments WHERE resolved = 0")?;
```

```typescript [TypeScript]
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL('./my-project', {
  tables: [
    new Table({
      ddl: 'CREATE TABLE comments (id TEXT, body TEXT, resolved INTEGER)',
      glob: 'comments/**/*.jsonl',
      extract: (path, content) =>
        content.split('\n').filter(Boolean).map(line => JSON.parse(line)),
    }),
  ],
});
await db.ready;

const unresolved = await db.query(
  'SELECT * FROM comments WHERE resolved = 0'
);
```

:::
