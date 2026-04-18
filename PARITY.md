# SDK Parity

API surface comparison across the three language SDKs.

## Core Types

| Concept     | Python                  | Rust                     | TypeScript               |
|-------------|-------------------------|--------------------------|--------------------------|
| Table def   | `Table(ddl, glob, extract, strict)` | `Table::new(ddl, glob, extract)` / `Table::strict(...)` / `Table::try_new(...)` | `{ ddl, glob, extract, strict? }` (plain object) |
| Row event   | `RowEvent` (class, frozen attrs; `file_path` on all variants) | `RowEvent` (enum: Insert/Update/Delete/Error; `file_path` on all variants) | `RowEvent` (plain object with action string; `filePath` on all variants) |
| Row type    | `dict[str, Any]`        | `HashMap<String, Value>` | `Record<string, unknown>` |

## DirSQL (synchronous)

| API                        | Python                           | Rust                              | TypeScript                        |
|----------------------------|----------------------------------|-----------------------------------|-----------------------------------|
| Constructor                | `DirSQL(root, *, tables, ignore)` | `DirSQL::new(root, tables)` / `DirSQL::with_ignore(root, tables, ignore)` | `new DirSQL(root, tables, ignore?)` |
| From config                | `DirSQL.from_config(path)`       | `DirSQL::from_config(root_dir)` / `DirSQL::from_config_path(cfg_path)` | `DirSQL.fromConfig(configPath)`   |
| Query (read-only; rejects non-SELECT) | `db.query(sql) -> list[dict]`    | `db.query(sql) -> Result<Vec<Row>>` | `await db.query(sql) -> Record[]` (runs on libuv threadpool) |
| Start watcher              | `db._start_watcher()`            | `db.start_watching()`             | `await db.startWatcher()` (runs on libuv threadpool) |
| Poll events                | `db._poll_events(ms)`            | `db.poll_events(duration)`        | `await db.pollEvents(ms)` (runs on libuv threadpool) |
| Watch (channel/stream)     | `async for event in db.watch()` (via `_async.py`) | `db.watch() -> WatchStream` (channel) | `for await (const ev of db.watch())` |

All three bindings share a single Rust implementation: `dirsql::DirSQL` handles
the initial scan, SQL, watcher, and row diffing. Python (`dirsql-py-ext`) and
TypeScript (`dirsql-napi`) bindings are thin shims that only marshal values
between the host language and Rust.

## AsyncDirSQL

| API                        | Python                                | Rust                                   |
|----------------------------|---------------------------------------|----------------------------------------|
| Constructor                | `AsyncDirSQL(root, *, tables, ignore)` | `AsyncDirSQL::new(root, tables)?` / `with_ignore(...)` |
| From config                | `AsyncDirSQL.from_config(path)`       | `AsyncDirSQL::from_config(root_dir)?`  |
| Ready                      | `await db.ready()`                    | `db.ready().await?`                    |
| Query                      | `await db.query(sql)`                 | `db.query(sql).await?`                 |
| Watch                      | `async for event in db.watch()`       | `db.watch()? -> WatchStream` (Stream trait) |

**TypeScript note:** JS is async by default, so there is no separate `AsyncDirSQL` class.
The single `DirSQL` class has `ready: Promise<void>` (an awaitable property) and
`watch(): AsyncIterable<RowEvent>` built in.  Usage:

```ts
const db = new DirSQL(root, tables);
await db.ready;
const rows = db.query("SELECT ...");
for await (const event of db.watch()) { ... }
```

## Language-Idiomatic Exceptions

### Python
- Uses `snake_case` for all identifiers.
- `Table` is a class with keyword-only constructor args.
- `RowEvent` is a frozen class with attribute access (`event.action`, `event.row`).
- `AsyncDirSQL` is a pure-Python wrapper using `asyncio.to_thread`.
- Watch low-level methods are prefixed with `_` (private convention).

### Rust
- Uses `snake_case` for all identifiers.
- `Table` has separate constructors: `new` (infallible extract), `try_new` (fallible extract), `strict` (shorthand).
- `RowEvent` is a Rust enum with variants (`Insert { table, row, file_path }`, `Update { table, old_row, new_row, file_path }`, `Delete { table, row, file_path }`, `Error { table, file_path, error }`) rather than a flat struct. `file_path` is a relative `String` on Insert/Update/Delete and a `PathBuf` on Error. `table` is `String` on Insert/Update/Delete and `Option<String>` on Error — `None` for errors that aren't tied to a specific table (e.g. a watch-channel failure). Python exposes the same field as `Optional[str]`; TypeScript as `string | null`.
- `DirSQL::from_config` takes a root directory path (looks for `.dirsql.toml` inside), not the config file path directly.
- `AsyncDirSQL` uses tokio and `OnceCell` internally.
- Watch returns `futures_channel::mpsc::UnboundedReceiver<RowEvent>` implementing `Stream`.
- All fallible operations return `Result<T, DirSqlError>`. Statements classified as writes by SQLite's `sqlite3_stmt_readonly` surface as the unit variant `DirSqlError::WriteForbidden`; in the Python/TS bindings the same condition is a `RuntimeError` / `Error` with a "read-only" message.

### TypeScript
- Uses `camelCase` for method names (`fromConfig`).
- `RowEvent` field names use `camelCase` (`oldRow`, `filePath`), not `snake_case`.
- Table definitions are plain objects (`{ ddl, glob, extract, strict? }`), not a class.
- `DirSQL.fromConfig` takes the config file path directly (like Python), not the root directory (like Rust).
- No separate `AsyncDirSQL` — JS is async by default, so `DirSQL` has `ready: Promise<void>`, `query(): Promise<Record[]>`, and `watch(): AsyncIterable<RowEvent>` built in.
- `query()`, `startWatcher()`, and `pollEvents()` all return `Promise`s and run on the libuv threadpool so the JS event loop stays responsive (even for long poll timeouts).
- The initial directory scan currently runs synchronously inside the constructor, so `ready` resolves immediately (and construction errors throw synchronously). The Promise exists so consumers can write uniform async-style code across SDKs. Tracked in #146.

## Test Coverage Matrix

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Basic init + query         | Y      | Y    | Y          |
| Multiple tables            | Y      | Y    | Y          |
| Ignore patterns            | Y      | Y    | Y          |
| from_config / fromConfig   | Y      | Y    | Y          |
| Watch: insert              | Y      | Y    | Y          |
| Watch: delete              | Y      | Y    | Y          |
| Watch: update              | Y      | Y    | Y          |
| Watch: error               | Y      | Y    | Y          |
| Query rejects writes       | Y      | Y    | Y          |
| Relaxed schema (extra keys)| Y      | Y    | Y          |
| Relaxed schema (missing)   | Y      | Y    | Y          |
| Strict mode (extra keys)   | Y      | Y    | Y          |
| Strict mode (missing keys) | Y      | Y    | Y          |
| Strict mode (exact match)  | Y      | Y    | Y          |
| AsyncDirSQL: ready + query | Y      | Y    | Y (via DirSQL.ready) |
| AsyncDirSQL: multiple ready| Y      | Y    | Y (via DirSQL.ready) |
| AsyncDirSQL: from_config   | Y      | Y    | Y (via DirSQL.fromConfig + ready) |
| AsyncDirSQL: watch         | Y      | Y    | Y (via DirSQL.watch) |
