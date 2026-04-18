# API Reference

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
DirSQL(root: str, *, tables: list[Table], ignore: list[str] | None = None)
```

```rust [Rust]
DirSQL::new(root: &str, tables: Vec<Table>) -> Result<DirSQL>
```

```typescript [TypeScript]
new DirSQL(root: string, tables: TableDef[], ignore?: string[])
```

:::

Creates an in-memory SQLite index over the given directory. In Python, the constructor starts scanning in a background thread and returns immediately. Call `await db.ready()` before querying. In Rust, the constructor scans synchronously. In TypeScript, scanning starts immediately.

**Parameters:**

- `root` -- Path to the directory to index.
- `tables` -- List of `Table` definitions. Each defines a SQLite table, a glob pattern, and an extract function.
- `ignore` -- Optional list of glob patterns. Files matching any ignore pattern are skipped regardless of table globs.

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
db.query(sql: string): Record<string, unknown>[]
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

#### `from_config`

::: code-group

```python [Python]
DirSQL.from_config(path: str) -> DirSQL
```

```rust [Rust]
DirSQL::from_config(path: &str) -> Result<DirSQL>
```

:::

Create a `DirSQL` instance from a `.dirsql.toml` config file. In Python, returns immediately and scans in the background -- call `await db.ready()` before querying.

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
Table(*, ddl: str, glob: str, extract: Callable[[str, str], list[dict]])
```

```rust [Rust]
Table::new(ddl: &str, glob: &str, extract: fn(&str, &str) -> Vec<Value>)
```

```typescript [TypeScript]
new Table({ ddl: string, glob: string, extract: (path: string, content: string) => Record<string, unknown>[] })
```

:::

Defines a mapping from files to SQLite table rows.

**Parameters:**

- `ddl` -- A `CREATE TABLE` statement. The table name is parsed from this DDL.
- `glob` -- A glob pattern matched against file paths relative to the root directory.
- `extract` -- A callable `(path, content) -> list[dict]`. Receives the relative file path and file content as strings. Returns a list of dicts/maps mapping column names to values. Return an empty list to skip a file.

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
