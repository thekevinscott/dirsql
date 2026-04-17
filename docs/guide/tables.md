# Defining Tables

Each table in `dirsql` maps a set of files to rows in an in-memory SQLite table. A table definition has three parts: DDL, a glob pattern, and an extract function.

## Table constructor

::: code-group

```python [Python]
from dirsql import Table

table = Table(
    ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
    glob="comments/**/index.jsonl",
    extract=lambda path, content: [
        {"id": "...", "body": "...", "author": "..."}
    ],
)
```

```rust [Rust]
use dirsql::{Table, Value};
use std::collections::HashMap;

let table = Table::new(
    "CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
    "comments/**/index.jsonl",
    |_path, _content| {
        let mut row: HashMap<String, Value> = HashMap::new();
        row.insert("id".into(), Value::Text("...".into()));
        row.insert("body".into(), Value::Text("...".into()));
        row.insert("author".into(), Value::Text("...".into()));
        vec![row]
    },
);
```

```typescript [TypeScript]
import type { TableDef } from 'dirsql';

const table: TableDef = {
  ddl: 'CREATE TABLE comments (id TEXT, body TEXT, author TEXT)',
  glob: 'comments/**/index.jsonl',
  extract: (_path, content) => [
    { id: '...', body: '...', author: '...' },
  ],
};
```

:::

All three arguments are keyword-only (in Python). In Rust they are positional to `Table::new`. In TypeScript a table is a plain `TableDef` object literal — the TS SDK exports the `TableDef` type (not a class).

### `ddl`

A SQLite `CREATE TABLE` statement. This defines the schema of the table. `dirsql` executes this DDL directly against the in-memory database, so any valid SQLite column types and constraints work.

```python
# Simple text columns
ddl="CREATE TABLE notes (title TEXT, body TEXT)"

# Typed columns
ddl="CREATE TABLE metrics (name TEXT, value REAL, count INTEGER)"

# With constraints
ddl="CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)"
```

The table name is parsed from the DDL. It must be a valid SQLite identifier.

### `glob`

A glob pattern that determines which files feed into this table. Matched relative to the root directory passed to `DirSQL`.

```python
glob="*.json"                  # JSON files in root only
glob="**/*.json"               # JSON files at any depth
glob="comments/**/index.jsonl" # JSONL files in comment subdirectories
glob="data/*.csv"              # CSV files in data/
```

Glob syntax follows standard Unix globbing rules. `**` matches any number of directory levels.

### `extract`

A callable `(path: str, content: str) -> list[dict]` that converts a file into rows.

- `path` is the file path relative to the root directory
- `content` is the file content as a string
- Return a list of dicts, where each dict maps column names to values
- Return an empty list to skip a file

```python
import json

# Single-object JSON files: one row per file
extract=lambda path, content: [json.loads(content)]

# JSONL files: one row per line
extract=lambda path, content: [
    json.loads(line) for line in content.splitlines()
]

# Derive values from the file path
import os
extract=lambda path, content: [
    {
        "id": os.path.basename(os.path.dirname(path)),
        "body": json.loads(line)["body"],
    }
    for line in content.splitlines()
    for _ in [json.loads(line)]
]

# Conditionally skip files
def extract(path, content):
    data = json.loads(content)
    if data.get("draft"):
        return []
    return [data]
```

## Multiple tables

Pass multiple `Table` definitions to index different file types into separate tables:

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./workspace",
    tables=[
        Table(
            ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
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
```

```rust [Rust]
use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;

// See `row_from_json` in getting-started.md for a reusable helper.
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
    "./workspace",
    vec![
        Table::new(
            "CREATE TABLE posts (title TEXT, author_id TEXT)",
            "posts/*.json",
            |_path, content| vec![row_from_json(content)],
        ),
        Table::new(
            "CREATE TABLE authors (id TEXT, name TEXT)",
            "authors/*.json",
            |_path, content| vec![row_from_json(content)],
        ),
    ],
)?;
```

```typescript [TypeScript]
import { DirSQL, type TableDef } from 'dirsql';

const tables: TableDef[] = [
  {
    ddl: 'CREATE TABLE posts (title TEXT, author_id TEXT)',
    glob: 'posts/*.json',
    extract: (_path, content) => [JSON.parse(content)],
  },
  {
    ddl: 'CREATE TABLE authors (id TEXT, name TEXT)',
    glob: 'authors/*.json',
    extract: (_path, content) => [JSON.parse(content)],
  },
];

const db = new DirSQL('./workspace', tables);
```

:::

Each table has its own glob and extract function. A file can only match one table (the first matching glob wins).

## Ignore patterns

Use the `ignore` parameter to exclude paths from all tables:

::: code-group

```python [Python]
db = DirSQL(
    "./workspace",
    ignore=["**/node_modules/**", "**/.git/**"],
    tables=[...],
)
```

```rust [Rust]
let db = DirSQL::with_ignore(
    "./workspace",
    vec![/* tables */],
    vec!["**/node_modules/**", "**/.git/**"],
)?;
```

```typescript [TypeScript]
const db = new DirSQL('./workspace', {
  ignore: ['**/node_modules/**', '**/.git/**'],
  tables: [...],
});
```

:::

Ignore patterns are applied before glob matching. Any file matching an ignore pattern is skipped regardless of table globs.

## Supported value types

The extract function can return these Python types, which map to SQLite types:

| Python type | SQLite type |
|-------------|-------------|
| `str`       | TEXT        |
| `int`       | INTEGER     |
| `float`     | REAL        |
| `bool`      | INTEGER (0/1) |
| `bytes`     | BLOB        |
| `None`      | NULL        |

Any other type is converted to its string representation via `str()`.
