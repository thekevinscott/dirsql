---
canonical: https://thekevinscott.github.io/dirsql/guide/tables
---

# Defining Tables

> Online: <https://thekevinscott.github.io/dirsql/guide/tables>

Each table in `dirsql` maps a set of files to rows in an in-memory SQLite table. A table definition has four parts: a name, a list of column definitions, a glob pattern, and an extract function.

## Table constructor

::: code-group

```python [Python]
from dirsql import Table

table = Table(
    name="comments",
    glob="comments/**/index.jsonl",
    columns=[
        {"name": "id", "type": "TEXT"},
        {"name": "body", "type": "TEXT"},
        {"name": "author", "type": "TEXT"},
    ],
    extract=lambda path: [
        {"id": "...", "body": "...", "author": "..."}
    ],
)
```

```rust [Rust]
use dirsql::{Column, ColumnType, Table, Value};
use std::collections::HashMap;

let table = Table::from_columns(
    "comments",
    "comments/**/index.jsonl",
    vec![
        Column::new("id", ColumnType::Text),
        Column::new("body", ColumnType::Text),
        Column::new("author", ColumnType::Text),
    ],
    |_path| {
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
  name: 'comments',
  glob: 'comments/**/index.jsonl',
  columns: [
    { name: 'id', type: 'TEXT' },
    { name: 'body', type: 'TEXT' },
    { name: 'author', type: 'TEXT' },
  ],
  extract: (_path) => [
    { id: '...', body: '...', author: '...' },
  ],
};
```

:::

The arguments are keyword-only (in Python). In Rust use the `Table::from_columns(name, glob, columns, extract)` constructor. In TypeScript a table is a plain `TableDef` object literal — the TS SDK exports the `TableDef` type (not a class).

::: tip Deprecated: `ddl`
Earlier versions defined a table with a raw `ddl="CREATE TABLE ..."` string instead of `name` + `columns`. The `ddl` form still works but is **deprecated** and slated for removal; new code should use the structured `columns` shape described here. The two forms are mutually exclusive — pass one or the other, not both.
:::

### `name`

The SQLite table name. It must be a valid SQLite identifier.

### `columns`

A list of column definitions that describe the table's schema. `dirsql` builds the `CREATE TABLE` statement for you from these definitions, so you never hand-write DDL. Each column is a plain dict (Python), object (TypeScript), or `Column` struct (Rust).

A column has a `name` and a `type` (one of `TEXT`, `INTEGER`, `REAL`, `BLOB`, `NUMERIC`):

```python
# Simple text columns
columns=[
    {"name": "title", "type": "TEXT"},
    {"name": "body", "type": "TEXT"},
]

# Mixed types
columns=[
    {"name": "name", "type": "TEXT"},
    {"name": "value", "type": "REAL"},
    {"name": "count", "type": "INTEGER"},
]
```

The storage-type strings are exported as constants in Python (`from dirsql import TEXT, INTEGER, REAL, BLOB, NUMERIC`); in Rust they are the `ColumnType` enum variants (`ColumnType::Text`, `ColumnType::Integer`, `ColumnType::Real`, `ColumnType::Blob`, `ColumnType::Numeric`); in TypeScript they are the `ColumnType` string-union type (`"TEXT" | "INTEGER" | "REAL" | "BLOB" | "NUMERIC"`).

#### Column constraints

A column may carry per-column constraints alongside `name` and `type`:

- `not_null` (`notNull` in TS) — `NOT NULL`
- `primary_key` (`primaryKey` in TS) — single-column `PRIMARY KEY`
- `unique` — `UNIQUE`
- `autoincrement` — `AUTOINCREMENT` (only meaningful on an `INTEGER PRIMARY KEY`)
- `collate` — a collation name, e.g. `"NOCASE"`
- `default` — a default value (see below)

::: code-group

```python [Python]
columns=[
    {"name": "id", "type": "INTEGER", "primary_key": True, "autoincrement": True},
    {"name": "slug", "type": "TEXT", "not_null": True, "unique": True},
    {"name": "title", "type": "TEXT", "not_null": True, "default": "untitled"},
    {"name": "email", "type": "TEXT", "collate": "NOCASE"},
]
```

```rust [Rust]
use dirsql::{Column, ColumnType, DefaultValue};

vec![
    Column {
        name: "id".into(),
        ty: ColumnType::Integer,
        primary_key: true,
        autoincrement: true,
        ..Default::default()
    },
    Column {
        name: "slug".into(),
        ty: ColumnType::Text,
        not_null: true,
        unique: true,
        ..Default::default()
    },
    Column {
        name: "title".into(),
        ty: ColumnType::Text,
        not_null: true,
        default: Some(DefaultValue::Text("untitled".into())),
        ..Default::default()
    },
    Column {
        name: "email".into(),
        ty: ColumnType::Text,
        collate: Some("NOCASE".into()),
        ..Default::default()
    },
]
```

```typescript [TypeScript]
columns: [
  { name: 'id', type: 'INTEGER', primaryKey: true, autoincrement: true },
  { name: 'slug', type: 'TEXT', notNull: true, unique: true },
  { name: 'title', type: 'TEXT', notNull: true, default: 'untitled' },
  { name: 'email', type: 'TEXT', collate: 'NOCASE' },
]
```

:::

#### Defaults

`default` accepts a scalar literal (string, number, boolean, or null). To use a SQL expression as the default — `CURRENT_TIMESTAMP`, `strftime(...)`, etc. — pass `{ sql: "..." }` instead of a bare value; it renders as `DEFAULT (<sql>)`.

::: code-group

```python [Python]
columns=[
    {"name": "title", "type": "TEXT", "default": "untitled"},  # literal
    {"name": "created", "type": "INTEGER", "default": {"sql": "strftime('%s', 'now')"}},  # expression
]
```

```rust [Rust]
use dirsql::{Column, ColumnType, DefaultValue};

vec![
    Column {
        name: "title".into(),
        ty: ColumnType::Text,
        default: Some(DefaultValue::Text("untitled".into())),
        ..Default::default()
    },
    Column {
        name: "created".into(),
        ty: ColumnType::Integer,
        default: Some(DefaultValue::Sql("strftime('%s', 'now')".into())),
        ..Default::default()
    },
]
```

```typescript [TypeScript]
columns: [
  { name: 'title', type: 'TEXT', default: 'untitled' }, // literal
  { name: 'created', type: 'INTEGER', default: { sql: "strftime('%s', 'now')" } }, // expression
]
```

:::

#### Check constraints and generated columns

The `{ sql: "..." }` escape hatch also drives `check` and `generated` columns:

- `check` — `{ sql: "..." }` renders a `CHECK (<sql>)` constraint on the column.
- `generated` — `{ sql: "...", mode?: "stored" | "virtual" }` renders a `GENERATED ALWAYS AS (<sql>)` column. `mode` defaults to `virtual`.

::: code-group

```python [Python]
columns=[
    {"name": "price", "type": "REAL", "check": {"sql": "price >= 0"}},
    {"name": "qty", "type": "INTEGER"},
    {"name": "total", "type": "REAL", "generated": {"sql": "price * qty", "mode": "stored"}},
]
```

```rust [Rust]
use dirsql::{Column, ColumnType, Expression, GeneratedColumn};

vec![
    Column {
        name: "price".into(),
        ty: ColumnType::Real,
        check: Some(Expression { sql: "price >= 0".into() }),
        ..Default::default()
    },
    Column::new("qty", ColumnType::Integer),
    Column {
        name: "total".into(),
        ty: ColumnType::Real,
        generated: Some(GeneratedColumn {
            sql: "price * qty".into(),
            mode: Some("stored".into()),
        }),
        ..Default::default()
    },
]
```

```typescript [TypeScript]
columns: [
  { name: 'price', type: 'REAL', check: { sql: 'price >= 0' } },
  { name: 'qty', type: 'INTEGER' },
  { name: 'total', type: 'REAL', generated: { sql: 'price * qty', mode: 'stored' } },
]
```

:::

### Table-level options

Beyond columns, a table accepts several optional table-level settings:

- `primary_key` (`primaryKey` in TS) — a list of column names for a **composite** primary key.
- `unique` — a list of column-name lists, each producing a composite `UNIQUE` constraint.
- `indexes` — a list of `{ name?, columns, unique? }` index definitions.
- `without_rowid` (`withoutRowid` in TS) — emit a `WITHOUT ROWID` table.
- `strict_types` (`strictTypes` in TS) — emit a SQLite [`STRICT`](https://www.sqlite.org/stricttables.html) table, enforcing column types at write time.

::: code-group

```python [Python]
from dirsql import Table

table = Table(
    name="memberships",
    glob="memberships/*.json",
    columns=[
        {"name": "user_id", "type": "TEXT", "not_null": True},
        {"name": "group_id", "type": "TEXT", "not_null": True},
        {"name": "role", "type": "TEXT"},
    ],
    primary_key=["user_id", "group_id"],
    unique=[["user_id", "group_id"]],
    indexes=[{"name": "idx_role", "columns": ["role"]}],
    without_rowid=True,
    strict_types=True,
    extract=lambda path: [...],
)
```

```rust [Rust]
use dirsql::{Column, ColumnType, Index, Table};

let mut table = Table::from_columns(
    "memberships",
    "memberships/*.json",
    vec![
        Column { name: "user_id".into(), ty: ColumnType::Text, not_null: true, ..Default::default() },
        Column { name: "group_id".into(), ty: ColumnType::Text, not_null: true, ..Default::default() },
        Column::new("role", ColumnType::Text),
    ],
    |_path| vec![/* ... */],
);
table.primary_key = vec!["user_id".into(), "group_id".into()];
table.unique = vec![vec!["user_id".into(), "group_id".into()]];
table.indexes = vec![Index {
    name: Some("idx_role".into()),
    columns: vec!["role".into()],
    unique: false,
}];
table.without_rowid = true;
table.strict_types = true;
```

```typescript [TypeScript]
import type { TableDef } from 'dirsql';

const table: TableDef = {
  name: 'memberships',
  glob: 'memberships/*.json',
  columns: [
    { name: 'user_id', type: 'TEXT', notNull: true },
    { name: 'group_id', type: 'TEXT', notNull: true },
    { name: 'role', type: 'TEXT' },
  ],
  primaryKey: ['user_id', 'group_id'],
  unique: [['user_id', 'group_id']],
  indexes: [{ name: 'idx_role', columns: ['role'] }],
  withoutRowid: true,
  strictTypes: true,
  extract: (_path) => [...],
};
```

:::

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

A callable `(path: str) -> list[dict]` that converts a file into rows.

- `path` is the **absolute filesystem path** of the matched file
- Return a list of dicts, where each dict maps column names to values
- Return an empty list to skip a file

`dirsql` does not read file contents for you. If your extract needs the file
body, read it inside the callback using `path`. Callbacks that derive columns
only from the path (or that rely solely on the auto-injected filesystem-fact
columns) never touch the file at all.

```python
import json

# Single-object JSON files: one row per file
def extract(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(f.read())]

# JSONL files: one row per line
def extract(path):
    with open(path, encoding="utf-8") as f:
        return [json.loads(line) for line in f]

# Derive a value from the file path alone -- no file read
import os
extract = lambda path: [{"id": os.path.basename(os.path.dirname(path))}]

# Conditionally skip files
def extract(path):
    with open(path, encoding="utf-8") as f:
        data = json.loads(f.read())
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
            name="posts",
            glob="posts/*.json",
            columns=[
                {"name": "title", "type": "TEXT"},
                {"name": "author_id", "type": "TEXT"},
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
```

```rust [Rust]
use dirsql::{Column, ColumnType, DirSQL, Table, Value};
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
        Table::from_columns(
            "posts",
            "posts/*.json",
            vec![
                Column::new("title", ColumnType::Text),
                Column::new("author_id", ColumnType::Text),
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
```

```typescript [TypeScript]
import { DirSQL, type TableDef } from 'dirsql';
import { readFileSync } from 'node:fs';

const tables: TableDef[] = [
  {
    name: 'posts',
    glob: 'posts/*.json',
    columns: [
      { name: 'title', type: 'TEXT' },
      { name: 'author_id', type: 'TEXT' },
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

const db = new DirSQL({ root: './workspace', tables });
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
const db = new DirSQL({
  root: './workspace',
  tables: [/* tables */],
  ignore: ['**/node_modules/**', '**/.git/**'],
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
