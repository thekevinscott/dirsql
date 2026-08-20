---
layout: home
hero:
  name: dirsql
  tagline: Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an ephemeral SQLite database, and exposes a SQL query interface. The filesystem is always the source of truth.
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

`dirsql` bridges this gap. The filesystem remains the source of truth, but you get SQL queries and real-time change events for free. Define tables with glob patterns and on-file callbacks, and `dirsql` handles the rest.

**`dirsql` never modifies your files.** It opens them for reading and nothing else — no writes, no moves, no deletes, no rewrites in place. Point it at anything and the worst it can do is read. This is permanent by design, not unimplemented; see [Read-only by design](./explanation#read-only-by-design) for its exact scope.

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-project",
    tables=[
        Table(
            name="records",
            ddl="CREATE TABLE records (name TEXT, size INTEGER, type TEXT)",
            glob="data/*.json",
            on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
        ),
    ],
)

# SQL queries over your filesystem
large = db.query("SELECT * FROM records WHERE size > 1000")
```

```rust [Rust]
use dirsql::{DirSQL, Table};

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::new(
            "records",
            "CREATE TABLE records (name TEXT, size INTEGER, type TEXT)",
            "data/*.json",
            |path| vec![serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()],
        ),
    ],
)?;

let large = db.query("SELECT * FROM records WHERE size > 1000")?;
```

```typescript [TypeScript]
import { readFileSync } from 'node:fs';
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL({
  root: './my-project',
  tables: [
    new Table({
      name: 'records',
      ddl: 'CREATE TABLE records (name TEXT, size INTEGER, type TEXT)',
      glob: 'data/*.json',
      onFile: (path) => [JSON.parse(readFileSync(path, 'utf8'))],
    }),
  ],
});

const large = await db.query('SELECT * FROM records WHERE size > 1000');
```

:::
