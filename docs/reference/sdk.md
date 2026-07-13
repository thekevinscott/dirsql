# SDK

`dirsql` ships as a library for Python (PyPI: `dirsql`), TypeScript (npm:
`dirsql`), and Rust (crates.io: `dirsql`). All three are thin bindings over
one shared Rust core, so behavior is identical; only the calling conventions
differ.

## Install

::: code-group

```bash [Python]
uv add dirsql
```

```bash [TypeScript]
pnpm add dirsql
```

```bash [Rust]
cargo add dirsql
```

:::

## Import

::: code-group

```python [Python]
from dirsql import DirSQL, Table, RowEvent
```

```typescript [TypeScript]
import { DirSQL, Table } from "dirsql";
import type { TableDef, RowEvent, ExtensionSpec, DirSQLOptions } from "dirsql";
```

```rust [Rust]
use dirsql::{DirSQL, AsyncDirSQL, Table, Row, RowEvent, Value, Extension, DirSqlError};
```

:::

## `DirSQL`

### Constructor

::: code-group

```python [Python]
DirSQL(
    root: str | None = None,
    *,
    tables: list[Table] | None = None,
    ignore: list[str] | None = None,
    config: str | list[str] | None = None,
    persist: bool = False,
    persist_path: str | None = None,
    extensions: list[dict] | None = None,  # [{ "path": str, "entrypoint"?: str }]
)
```

```typescript [TypeScript]
new DirSQL(configPath: string)
// or
new DirSQL({
    root?: string,
    tables?: TableDef[],
    ignore?: string[],
    config?: string | string[],
    persist?: boolean,
    persistPath?: string,
    extensions?: ExtensionSpec[],  // [{ path: string, entrypoint?: string }]
})
```

```rust [Rust]
DirSQL::builder()
    .root(root)                     // optional
    .tables(tables)                 // optional; append one with .table(t)
    .ignore(patterns)               // optional
    .config(config_toml_path)       // optional; repeatable — call again to
                                    //   merge another config in call order
    .persist(cache_path)            // optional; Some(path), or None for the
                                    //   default <root>/.dirsql/cache.db
    .extensions(extensions)         // optional; append one with .extension(e)
    .poll_interval(duration)        // optional; watch-loop cadence, default 200ms
    .build()                        // -> Result<DirSQL>  (synchronous scan)
// Shortcuts: DirSQL::new(root, tables), DirSQL::with_ignore(root, tables, ignore),
//            DirSQL::from_config(root), DirSQL::from_config_path(path)
```

:::

Creates a SQLite index over a directory. The index root is the explicit
`root` when given, else the **process cwd** — the `config` file's location
never sets the root.

**Parameters:**

- `root` — Directory to index. When omitted, the index roots at the process
  cwd (even when `config` is supplied).
- `tables` — Programmatic [`Table`](#table) definitions.
- `ignore` — Glob patterns matched against root-relative paths; matched
  files are skipped entirely (scan and watch).
- `config` — Path to a [`.dirsql.toml`](./config.md). Its `[[table]]`
  entries are appended after any programmatic `tables`; its `ignore`
  patterns and `[[dirsql.extension]]` entries are appended likewise. The
  config file does **not** set the index root: with no explicit `root`, the
  index roots at the process cwd. **Multiple configs merge in order** on
  every SDK — **Python** `config` accepts a `str` or a `list[str]`, the
  **Rust** builder's `.config()` is repeatable (call it once per file), and
  **TypeScript** `config` accepts a `string` or a `string[]`; the configs
  merge in call order (`[[table]]` / `ignore` / `[[dirsql.extension]]`
  accumulate; a duplicate table name across configs errors), mirroring the
  CLI's repeatable [`-c/--config`](./cli.md#flags).
- `persist` — Keep the SQLite index on disk between runs (default off:
  ephemeral, rebuilt every startup). The cache lives at
  `<root>/.dirsql/cache.db` by default; on restart, only files whose stat
  changed are re-parsed. (The CLI exposes the same switch as the
  [`--persist [PATH]`](./cli.md#server-mode) flag; it is not a config key.)
- `persist_path` / `persistPath` — Override the cache location. Ignored when
  persistence is off. Constructor values are used as given. In Rust these two
  parameters collapse into a single builder method: `.persist(None)` enables
  persistence at the default location, `.persist(Some(path))` enables it at
  `path`.
- `extensions` — SQLite extensions to load at startup, before any table
  DDL (enable → load → disable, so SQL `load_extension()` is never
  exposed). Each entry pairs a shared-library `path` with an optional
  `entrypoint` init symbol. In Python and TypeScript, `path` may be a bare
  **package name**, resolved from the installed package (`importlib` /
  `node_modules`) — for both the programmatic list and a `config` file's
  `[[dirsql.extension]]` entries. The Rust SDK is file-path-only.
  Programmatic entries load first, then the config's. A relative
  programmatic path is used verbatim (resolved by the OS against the
  process working directory); config-file paths resolve against the config
  file's parent.

**Construction is asynchronous in Python and TypeScript.** The constructor
returns immediately and the initial scan runs in the background; `query`
and the other methods transparently await [readiness](#ready). In Rust,
`.build()` scans synchronously; use `.build_async()` → [`AsyncDirSQL`](#asyncdirsql-rust)
for the non-blocking equivalent.

::: details Rust builder extras
- `Table` appears in the builder via `.table(t)` (append) or `.tables(v)`
  (replace).
- `.poll_interval(Duration)` sets the channel-based `watch()` loop's poll
  cadence (default 200 ms): lower values give tighter event latency at
  higher idle CPU.
- `.suppress_config_extensions(true)` skips loading the config file's own
  `[[dirsql.extension]]` entries. Launcher plumbing (used with
  pre-resolved `.extensions(...)`); not needed in application code.
- `Extension { path: PathBuf, entrypoint: Option<String> }` is the
  extension spec type.
:::

### `ready`

::: code-group

```python [Python]
await db.ready() -> None
```

```typescript [TypeScript]
await db.ready  // awaitable property
```

```rust [Rust]
// AsyncDirSQL only; DirSQL::build() is already complete when it returns.
db.ready().await -> Result<()>
```

:::

Waits for the initial scan to complete, re-raising any construction error.
Safe to call multiple times. In Python and TypeScript every other method
awaits readiness internally, so an explicit `ready` is only needed to
observe construction errors before issuing a query.

### `query`

::: code-group

```python [Python]
await db.query(sql: str) -> list[dict]
```

```typescript [TypeScript]
await db.query(sql: string) -> Record<string, unknown>[]
// Runs on the libuv threadpool; the JS event loop stays responsive.
```

```rust [Rust]
db.query(sql: &str) -> Result<Vec<Row>>   // Row = HashMap<String, Value>
```

:::

Executes a SQL query and returns rows keyed by column name.

- **Read-only.** Each statement is classified by SQLite itself
  (`sqlite3_stmt_readonly`); anything classified as a write — `INSERT`,
  `UPDATE`, `DELETE`, `DROP`, `CREATE`, `ALTER`, `VACUUM`, … — is rejected
  before producing rows. Rust surfaces this as
  `DirSqlError::WriteForbidden`; Python raises a `RuntimeError` and
  TypeScript rejects with an `Error` carrying a "read-only" message.
- Internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) are
  excluded from `SELECT *` results; name them explicitly to see them.
- SQLite values map back to language types:

| SQLite | Python | TypeScript | Rust |
|---|---|---|---|
| TEXT | `str` | `string` | `Value::Text` |
| INTEGER | `int` | `number` | `Value::Integer` |
| REAL | `float` | `number` | `Value::Real` |
| BLOB | `bytes` | `Buffer` | `Value::Blob` |
| NULL | `None` | `null` | `Value::Null` |

An `INTEGER` outside JavaScript's safe integer range (magnitude greater
than `Number.MAX_SAFE_INTEGER`, 2^53 − 1) cannot be represented as a JS
`number` without precision loss, so the TypeScript SDK **throws** rather
than return a rounded value. Python's `int` is unbounded, so it always
round-trips.

### `watch`

::: code-group

```python [Python]
async for event in db.watch():   # AsyncIterator[RowEvent]
    ...
```

```typescript [TypeScript]
for await (const event of db.watch()) {   // AsyncIterable<RowEvent>
    ...
}
```

```rust [Rust]
use futures::StreamExt;
let mut stream = db.watch()?;   // WatchStream: impl Stream<Item = RowEvent>
while let Some(event) = stream.next().await { ... }
```

:::

Returns a stream of [`RowEvent`](#rowevent)s reflecting filesystem changes
under the root. The watcher starts on first iteration (Python/TypeScript)
or at the `watch()` call (Rust). The stream never terminates on its own;
stop consuming it to stop.

Rust: `watch()` may be called once per instance and is mutually exclusive
with the polling API below — mixing them returns an error, since both drain
the same underlying filesystem watcher.

### Low-level watch primitives

::: code-group

```typescript [TypeScript]
await db.startWatcher()               // idempotent; must precede pollEvents
await db.pollEvents(timeoutMs) -> RowEvent[]
```

```rust [Rust]
db.start_watching() -> Result<()>     // idempotent; implied by poll_events/watch
db.poll_events(timeout: Duration) -> Result<Vec<RowEvent>>
```

:::

`pollEvents` / `poll_events` blocks up to the timeout for the first event,
then drains everything that arrived, applies it to the database, and
returns the batch (possibly empty). Safe to call in a loop. TypeScript runs
the poll on the libuv threadpool. Python does not expose public polling
primitives — use `watch()`.

## `AsyncDirSQL` (Rust)

Rust's non-blocking wrapper: the constructor returns immediately while the
scan runs on a background thread. Python and TypeScript need no equivalent —
their `DirSQL` is already async-by-default.

```rust
let db = DirSQL::builder().root("./data").tables(tables).build_async()?; // -> AsyncDirSQL
db.ready().await?;                        // required before use
let rows = db.query("SELECT ...").await?; // spawn_blocking under the hood
let stream = db.watch()?;                 // same WatchStream as DirSQL
let sync: DirSQL = db.sync()?;            // unwrap the inner sync handle
```

Shortcuts mirror `DirSQL`: `AsyncDirSQL::new`, `with_ignore`,
`from_config`, `from_config_path`. Unlike the Python/TypeScript SDKs,
methods called before `ready().await` completes return a "not ready" error
rather than waiting — an intentional, language-idiomatic difference.

## `Table`

::: code-group

```python [Python]
Table(*, ddl: str, glob: str, on_file: Callable[[str], list[dict]], strict: bool = False)
```

```typescript [TypeScript]
new Table({ ddl, glob, onFile, strict? })
// or a plain object — TableDef and Table are interchangeable:
{ ddl: string, glob: string, onFile: (path: string) => Record<string, unknown>[], strict?: boolean }
```

```rust [Rust]
Table::new(ddl, glob, on_file)      // on_file: Fn(&str) -> Vec<Row>, infallible
Table::try_new(ddl, glob, on_file)  // on_file: Fn(&str) -> Result<Vec<Row>, _>
Table::strict(ddl, glob, on_file)   // Table::new with strict = true
```

:::

Maps files to table rows.

- `ddl` — A SQLite `CREATE TABLE` statement; the table name is parsed from
  it. Table names must be unique across all tables.
- `glob` — Glob pattern matched against root-relative paths. May contain
  `{name}` [captures](./columns.md#glob-captures). Every table whose glob
  matches a file receives that file's rows — a file can populate multiple
  tables.
- `on_file` (`on_file` / `onFile`) — Callback receiving the matched file's full path (the root
  joined with the file's relative path — absolute when `root` is absolute)
  and returning the rows that file contributes. `dirsql` never reads file
  contents itself; a callback that needs the body reads the path. Return
  an empty list to skip a file. [Virtual columns and glob
  captures](./columns.md) are merged onto each returned row; values the
  callback emits win over same-named facts.
- `strict` — Default off: extra row keys are dropped and missing declared
  columns become `NULL`. When on, any extra or missing key is an error
  (see [`strict`](./config.md#table)).

Readable attributes: `ddl`, `glob` (all SDKs), plus `strict`.

## `RowEvent`

Emitted by [`watch`](#watch) and the polling primitives; one event per
row-level change caused by a filesystem event.

| Field | Python | Rust | TypeScript |
|---|---|---|---|
| Action | `action: str` | enum variant (`Insert` / `Update` / `Delete` / `Error`) | `action: string` |
| Table | `table: str \| None` | `table: String` (`Option<String>` on `Error`) | `table: string \| null` |
| New/current row | `row: dict \| None` | `row` / `new_row` on the variant | `row?: Record \| null` |
| Previous row | `old_row: dict \| None` | `old_row` on `Update` | `oldRow?: Record \| null` |
| Error message | `error: str \| None` | `error: String` on `Error` | `error?: string \| null` |
| File path | `file_path: str \| None` | `file_path` (`String`; `PathBuf` on `Error`) | `filePath?: string \| null` |

Actions: `insert` (new row; no previous), `update` (row changed in place;
carries both old and new), `delete` (row removed), `error` (an extraction
or watch failure — carries `error`, never `row`/`old_row`; `table` is
`None`/`null` when the failure isn't tied to one table). `file_path` is
relative to the root. Errors never terminate the stream.

## Value types

An `on_file` callback (and the SQLite columns it feeds) accepts:

| SQLite column | Python value | TypeScript value | Rust `Value` |
|---|---|---|---|
| TEXT | `str` | `string` | `Value::Text(String)` |
| INTEGER | `int`, `bool` | `number`, `boolean`, `bigint` | `Value::Integer(i64)` |
| REAL | `float` | `number` | `Value::Real(f64)` |
| BLOB | `bytes` | `Buffer` / `Uint8Array` | `Value::Blob(Vec<u8>)` |
| NULL | `None` | `null` | `Value::Null` |

Integers are stored as a signed 64-bit `Value::Integer`. A value that does
not fit that range is a hard error, not a lossy conversion: Python raises
`OverflowError`, and a TypeScript `bigint` outside the `i64` range throws.
A TypeScript `bigint` **within** `i64` range maps to `INTEGER`. Only a real
`bytes`/`bytearray` (Python) or `Buffer`/`Uint8Array` (TypeScript) maps to
`BLOB` — a list/array of integers does not.
