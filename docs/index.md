---
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
            ddl="CREATE TABLE files (name TEXT, size INTEGER, type TEXT)",
            glob="data/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ],
)

# SQL queries over your filesystem
large = db.query("SELECT * FROM files WHERE size > 1000")
```

```rust [Rust]
use dirsql::{DirSQL, Table};

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::new(
            "CREATE TABLE files (name TEXT, size INTEGER, type TEXT)",
            "data/*.json",
            |_path, content| vec![serde_json::from_str(content).unwrap()],
        ),
    ],
)?;

let large = db.query("SELECT * FROM files WHERE size > 1000")?;
```

```typescript [TypeScript]
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL('./my-project', {
  tables: [
    new Table({
      ddl: 'CREATE TABLE files (name TEXT, size INTEGER, type TEXT)',
      glob: 'data/*.json',
      extract: (_path, content) => [JSON.parse(content)],
    }),
  ],
});

const large = await db.query('SELECT * FROM files WHERE size > 1000');
```

:::
