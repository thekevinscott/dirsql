---
canonical: https://thekevinscott.github.io/dirsql/
layout: home
hero:
  name: dirsql
  tagline: Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. The filesystem is always the source of truth.
  actions:
    - theme: brand
      text: Get Started
      link: /getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/thekevinscott/dirsql
---

Structured data stored as flat files (JSON, CSV, markdown) is easy to read, write, diff, and version-control.

But querying across many files is slow.

"Show me all records matching X across 50 files" requires opening and parsing every file.

## Solution

`dirsql` bridges this gap. The filesystem remains the source of truth, but you get SQL queries and real-time change events for free. Define tables with glob patterns and extract functions, and `dirsql` handles the rest.

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-project",
    tables=[
        Table(
            name="files",
            glob="data/*.json",
            columns=[
                {"name": "name", "type": "TEXT"},
                {"name": "size", "type": "INTEGER"},
                {"name": "type", "type": "TEXT"},
            ],
            extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
        ),
    ],
)

# SQL queries over your filesystem
large = db.query("SELECT * FROM files WHERE size > 1000")
```

```rust [Rust]
use dirsql::{Column, ColumnType, DirSQL, Table};

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::from_columns(
            "files",
            "data/*.json",
            vec![
                Column::new("name", ColumnType::Text),
                Column::new("size", ColumnType::Integer),
                Column::new("type", ColumnType::Text),
            ],
            |path| vec![serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()],
        ),
    ],
)?;

let large = db.query("SELECT * FROM files WHERE size > 1000")?;
```

```typescript [TypeScript]
import { readFileSync } from 'node:fs';
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL({
  root: './my-project',
  tables: [
    new Table({
      name: 'files',
      glob: 'data/*.json',
      columns: [
        { name: 'name', type: 'TEXT' },
        { name: 'size', type: 'INTEGER' },
        { name: 'type', type: 'TEXT' },
      ],
      extract: (path) => [JSON.parse(readFileSync(path, 'utf8'))],
    }),
  ],
});

const large = await db.query('SELECT * FROM files WHERE size > 1000');
```

:::
