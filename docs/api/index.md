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
Table(
    *,
    name: str,
    glob: str,
    columns: list[dict],
    extract: Callable[[str], list[dict]],
    primary_key: list[str] | None = None,
    unique: list[list[str]] | None = None,
    indexes: list[dict] | None = None,
    without_rowid: bool = False,
    strict_types: bool = False,
)
```

```rust [Rust]
Table::from_columns(
    name: &str,
    glob: &str,
    columns: Vec<Column>,
    extract: fn(&str) -> Vec<Value>,
) -> Table
// Table-level options (primary_key, unique, indexes, without_rowid,
// strict_types) are public fields set on the returned value.
```

```typescript [TypeScript]
new Table({
    name: string,
    glob: string,
    columns: ColumnDef[],
    extract: (path: string) => Record<string, unknown>[],
    primaryKey?: string[],
    unique?: string[][],
    indexes?: IndexDef[],
    withoutRowid?: boolean,
    strictTypes?: boolean,
})
```

:::

Defines a mapping from files to SQLite table rows. `dirsql` builds the `CREATE TABLE` statement from `name` and `columns`.

**Parameters:**

- `name` -- The SQLite table name. Must be a valid SQLite identifier.
- `glob` -- A glob pattern matched against file paths relative to the root directory.
- `columns` -- A list of column definitions (see [Column fields](#column-fields) below).
- `extract` -- A callable `(path) -> list[dict]`. Receives the absolute filesystem path of the matched file. `dirsql` does not read file contents; a callback that needs the file body reads `path` itself. Returns a list of dicts/maps mapping column names to values. Return an empty list to skip a file.

**Table-level options (all optional):**

- `primary_key` (`primaryKey` in TS) -- list of column names forming a composite `PRIMARY KEY`.
- `unique` -- list of column-name lists, each a composite `UNIQUE` constraint.
- `indexes` -- list of index definitions `{ name?, columns, unique? }`.
- `without_rowid` (`withoutRowid` in TS) -- emit a `WITHOUT ROWID` table.
- `strict_types` (`strictTypes` in TS) -- emit a SQLite `STRICT` table.

In Rust these are public fields on `Table` (set them after `Table::from_columns(...)`); their names are `primary_key`, `unique`, `indexes`, `without_rowid`, `strict_types`.

#### Column fields

Each entry in `columns` is a plain dict (Python), object (TypeScript), or `Column` struct (Rust):

- `name` -- column name (string).
- `type` -- one of `TEXT`, `INTEGER`, `REAL`, `BLOB`, `NUMERIC`. In Python these are exported as constants (`from dirsql import TEXT, INTEGER, REAL, BLOB, NUMERIC`); in Rust as the `ColumnType` enum; in TypeScript as the `ColumnType` string-union type.
- `not_null` (`notNull` in TS) -- boolean, `NOT NULL`.
- `primary_key` (`primaryKey` in TS) -- boolean, single-column `PRIMARY KEY`.
- `unique` -- boolean, `UNIQUE`.
- `autoincrement` -- boolean, `AUTOINCREMENT`.
- `collate` -- collation name (string), e.g. `"NOCASE"`.
- `default` -- a scalar/null literal, or `{ sql: "..." }` for an expression (renders `DEFAULT (<sql>)`).
- `check` -- `{ sql: "..." }`, renders `CHECK (<sql>)`.
- `generated` -- `{ sql: "...", mode?: "stored" | "virtual" }`, renders `GENERATED ALWAYS AS (<sql>)`.

In Rust, defaults are the `DefaultValue` enum (`DefaultValue::Text`, `DefaultValue::Integer`, `DefaultValue::Real`, `DefaultValue::Sql`, ...); `check` is `Expression { sql }`; `generated` is `GeneratedColumn { sql, mode }`; indexes are `Index { name, columns, unique }`.

::: tip Deprecated: `ddl`
A table may instead be defined with a raw `ddl="CREATE TABLE ..."` string (`Table::new(ddl, glob, extract)` in Rust). This form is **deprecated**, retained only for backward compatibility, and slated for removal. Use `name` + `columns` for new code.
:::

**Attributes:**

- `name` -- The table name (read-only).
- `columns` -- The column definitions (read-only).
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
