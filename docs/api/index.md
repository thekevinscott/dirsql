---
canonical: https://thekevinscott.github.io/dirsql/api/
---

# API Reference

> Online: <https://thekevinscott.github.io/dirsql/api/>

## DirSQL

### Import

::: code-group

```python [Python]
from dirsql import DirSQL
```

```rust [Rust]
use dirsql::DirSQL;
```

```typescript [TypeScript]
import { DirSQL } from 'dirsql';
```

:::

### Constructor

::: code-group

```python [Python]
DirSQL(
    root: str | None = None,
    *,
    tables: list[Table] | None = None,
    ignore: list[str] | None = None,
    config: str | None = None,
)
```

```rust [Rust]
DirSQL::builder()
    .root(root)                 // optional
    .tables(tables)             // optional; append with .table(t)
    .ignore(patterns)           // optional
    .config(config_toml_path)   // optional
    .build()                    // -> Result<DirSQL>
```

```typescript [TypeScript]
new DirSQL(configPath: string)
// or
new DirSQL({
    root?: string,
    tables?: TableDef[],
    ignore?: string[],
    config?: string,
})
```

:::

Creates an in-memory SQLite index over the given directory. At least one of `root` or `config` must be supplied.

When both `root` and `config` are supplied -- or when `config` declares `[dirsql].root` -- the explicit `root` wins and a warning is emitted on stderr. A `[dirsql].root` declared in the config file is resolved relative to the config file's parent directory.

In Python, the constructor starts scanning in a background thread and returns immediately. Call `await db.ready()` before querying. In Rust, `.build()` scans synchronously; use `.build_async()` (via `AsyncDirSQL`) for the tokio-driven equivalent. In TypeScript, scanning starts immediately and `db.ready` resolves when the scan finishes.

**Parameters:**

- `root` -- Path to the directory to index. Optional if `config` is supplied.
- `tables` -- List of `Table` definitions. Each defines a SQLite table, a glob pattern, and an extract function.
- `ignore` -- Optional list of glob patterns. Files matching any ignore pattern are skipped regardless of table globs.
- `config` -- Optional path to a `.dirsql.toml` config file. Its `[[table]]` entries are appended to any programmatic `tables`; its `[dirsql].ignore` patterns are appended to any explicit `ignore`; its optional `[dirsql].root` supplies the root directory when `root` is not passed explicitly.

### Methods

#### `ready`

::: code-group

```python [Python]
await db.ready() -> None
```

```rust [Rust]
db.ready().await -> Result<()>
```

```typescript [TypeScript]
await db.ready  // awaitable property
```

:::

Wait for the initial scan to complete. Re-raises any exception from the scan. Safe to call multiple times.

#### `query`

::: code-group

```python [Python]
await db.query(sql: str) -> list[dict]
```

```rust [Rust]
db.query(sql: &str) -> Result<Vec<HashMap<String, Value>>>
```

```typescript [TypeScript]
await db.query(sql: string): Promise<Record<string, unknown>[]>
```

:::

Execute a SQL query against the in-memory database. Returns results keyed by column name. Internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) are excluded from results.

#### `watch`

::: code-group

```python [Python]
async for event in db.watch():  # AsyncIterator[RowEvent]
    ...
```

```rust [Rust]
let mut stream = db.watch();  // impl Stream<Item = RowEvent>
while let Some(event) = stream.next().await { ... }
```

```typescript [TypeScript]
for await (const event of db.watch()) {  // AsyncIterable<RowEvent>
    ...
}
```

:::

Returns an async iterable of `RowEvent` objects. The file watcher starts automatically on first iteration. The iterator never terminates on its own.

#### Resolved-state serialization

Each SDK exposes the instance's resolved runtime state through the host
language's natural serialization hook. The shape is identical across all
three SDKs (modulo `persist_path` ↔ `persistPath` case): a serialized
payload can flow through the `interpret` handshake regardless of which
SDK produced it.

The serialized shape captures **resolved runtime state**, not
construction parameters. The original `config` path is excluded — by the
time the instance exists, the config file has been read and its contents
merged into `root`, `tables`, and `ignore`. Per-table `extract`
(closure / callable) and `name` (derivable from DDL) are also excluded.

::: code-group

```python [Python]
import json

db = DirSQL(root="./data", tables=[...])
await db.ready()

state = vars(db)
# {
#   "root": "./data",
#   "tables": [{"ddl": "...", "glob": "...", "strict": False}, ...],
#   "ignore": [],
#   "persist": False,
#   "persist_path": None,
# }

payload = json.dumps(state)
```

```rust [Rust]
use dirsql::{DirSQL, Table};

let db = DirSQL::builder().root("./data").build()?;
let cfg = db.config();
// DirSQLConfig {
//   root: PathBuf,
//   tables: Vec<TableConfig>,  // { ddl, glob, strict }
//   ignore: Vec<String>,
//   persist: bool,
//   persist_path: Option<PathBuf>,
// }

let payload = serde_json::to_string(&cfg)?;
```

```typescript [TypeScript]
const db = new DirSQL({ root: "./data", tables: [...] });
await db.ready;

const payload = JSON.stringify(db);
// {
//   "root": "./data",
//   "tables": [{"ddl": "...", "glob": "...", "strict": false}, ...],
//   "ignore": [],
//   "persist": false,
//   "persistPath": null
// }
```

:::

Python uses the standard `__dict__` property so `vars(db)` and
`json.dumps(vars(db))` both work. TypeScript uses the built-in `toJSON()`
hook so `JSON.stringify(db)` works directly. Rust uses
`serde::Serialize`-derived structs (`DirSQLConfig` and `TableConfig`) so
callers can plug into the wider serde ecosystem.

In Python and TypeScript, serialization runs synchronously and resolves
the construction inputs (reading the `.dirsql.toml` if `config=` was
supplied) without waiting for the directory scan — `vars(db)` and
`JSON.stringify(db)` work immediately after construction. In Rust, the
synchronous `build()` returns a ready instance, so `db.config()` is
always available; the async variant requires `AsyncDirSQL::ready().await`
first (since `config()` lives on the inner sync instance).

---

## Table

### Import

::: code-group

```python [Python]
from dirsql import Table
```

```rust [Rust]
use dirsql::Table;
```

```typescript [TypeScript]
import { Table } from 'dirsql';
```

:::

### Constructor

::: code-group

```python [Python]
Table(*, ddl: str, glob: str, extract: Callable[[str], list[dict]])
```

```rust [Rust]
Table::new(ddl: &str, glob: &str, extract: fn(&str) -> Vec<Value>)
```

```typescript [TypeScript]
new Table({ ddl: string, glob: string, extract: (path: string) => Record<string, unknown>[] })
```

:::

Defines a mapping from files to SQLite table rows.

**Parameters:**

- `ddl` -- A `CREATE TABLE` statement. The table name is parsed from this DDL.
- `glob` -- A glob pattern matched against file paths relative to the root directory.
- `extract` -- A callable `(path) -> list[dict]`. Receives the absolute filesystem path of the matched file. `dirsql` does not read file contents; a callback that needs the file body reads `path` itself. Returns a list of dicts/maps mapping column names to values. Return an empty list to skip a file.

**Attributes:**

- `ddl` -- The DDL string (read-only).
- `glob` -- The glob pattern (read-only).

---

## RowEvent

### Import

::: code-group

```python [Python]
from dirsql import RowEvent
```

```rust [Rust]
use dirsql::RowEvent;
```

```typescript [TypeScript]
import { RowEvent } from 'dirsql';
```

:::

Emitted by the watch stream. Represents a change to a row in the database caused by a filesystem event.

**Attributes:**

| Attribute | Python | Rust | TypeScript |
|-----------|--------|------|------------|
| Table name | `table: str` | `table: String` | `table: string` |
| Action | `action: str` | `action: Action` | `action: string` |
| Current/new row | `row: dict \| None` | `row: Option<HashMap>` | `row?: Record` |
| Previous row | `old_row: dict \| None` | `old_row: Option<HashMap>` | `oldRow?: Record` |
| Error message | `error: str \| None` | `error: Option<String>` | `error?: string` |
| File path | `file_path: str \| None` | `file_path: Option<String>` | `filePath?: string` |

Action values: `"insert"`, `"update"`, `"delete"`, `"error"`.
