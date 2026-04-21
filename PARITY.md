# SDK Parity

API surface comparison across the three language SDKs.

## Core Types

| Concept     | Python                  | Rust                     | TypeScript               |
|-------------|-------------------------|--------------------------|--------------------------|
| Table def   | `Table(ddl, glob, extract, strict)` | `Table::new(ddl, glob, extract)` / `Table::strict(...)` / `Table::try_new(...)` | `{ ddl, glob, extract, strict? }` (plain object) |
| Row event   | `RowEvent` (class, frozen attrs; `file_path` on all variants) | `RowEvent` (enum: Insert/Update/Delete/Error; `file_path` on all variants) | `RowEvent` (plain object with action string; `filePath` on all variants) |
| Row type    | `dict[str, Any]`        | `HashMap<String, Value>` | `Record<string, unknown>` |

## DirSQL (synchronous)

All three SDKs share a single unified construction entry point — no separate
`from_config` / `fromConfig` factory. Callers supply any combination of
`root`, `tables`, `ignore`, and `config`; `config` names a `.dirsql.toml`
file whose `[[table]]` entries are appended and whose optional
`[dirsql].root` is resolved relative to the config file. When both an
explicit `root` and a config-supplied root are present, the explicit value
wins (a warning is emitted on stderr).

| API                        | Python                                         | Rust                                                 | TypeScript                                              |
|----------------------------|------------------------------------------------|------------------------------------------------------|---------------------------------------------------------|
| Constructor                | `DirSQL(root=None, *, tables=None, ignore=None, config=None, persist=False, persist_path=None)` | `DirSQL::builder().root(..).tables(..).ignore(..).config(..).persist(..).persist_path(..).build()` (also `DirSQL::new`/`with_ignore` shortcuts) | `new DirSQL(configPath)` or `new DirSQL({ root?, tables?, ignore?, config?, persist?, persistPath? })` + `await db.ready` |
| Query (read-only; rejects non-SELECT) | `db.query(sql) -> list[dict]`        | `db.query(sql) -> Result<Vec<Row>>`                  | `await db.query(sql) -> Record[]` (runs on libuv threadpool) |
| Start watcher              | `db._start_watcher()`                          | `db.start_watching()`                                | `await db.startWatcher()` (runs on libuv threadpool)    |
| Poll events                | `db._poll_events(ms)`                          | `db.poll_events(duration)`                           | `await db.pollEvents(ms)` (runs on libuv threadpool)    |
| Watch (channel/stream)     | `async for event in db.watch()` (via `_async.py`) | `db.watch() -> WatchStream` (channel)                | `for await (const ev of db.watch())`                    |

All three bindings share a single Rust implementation: `dirsql::DirSQL` handles
the initial scan, SQL, watcher, and row diffing. Python (`dirsql-py-ext`) and
TypeScript (`dirsql-napi`) bindings are thin shims that only marshal values
between the host language and Rust.

## AsyncDirSQL

| API                        | Python                                | Rust                                   |
|----------------------------|---------------------------------------|----------------------------------------|
| Constructor                | (merged into `DirSQL`; the Python `DirSQL` is already async-by-default) | `AsyncDirSQL::builder().root(..).tables(..).ignore(..).config(..).persist(..).persist_path(..).build_async()?` (also `new`/`with_ignore` shortcuts) |
| Ready                      | `await db.ready()`                    | `db.ready().await?`                    |
| Query                      | `await db.query(sql)`                 | `db.query(sql).await?`                 |
| Watch                      | `async for event in db.watch()`       | `db.watch()? -> WatchStream` (Stream trait) |

**TypeScript note:** JS is async by default, so there is no separate `AsyncDirSQL` class.
The single `DirSQL` class has `ready: Promise<void>` (an awaitable property) and
`watch(): AsyncIterable<RowEvent>` built in.  Usage:

```ts
// From a config file:
const db = new DirSQL("./my-config.toml");
// Programmatic:
const db2 = new DirSQL({ root, tables });
await db.ready;
const rows = await db.query("SELECT ...");
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
- Construction uses a builder (`DirSQL::builder()...build()`); the `new`/`with_ignore`/`from_config`/`from_config_path` shortcuts remain as thin wrappers delegating to the builder.
- `AsyncDirSQL` uses tokio and `OnceCell` internally.
- Watch returns `futures_channel::mpsc::UnboundedReceiver<RowEvent>` implementing `Stream`.
- All fallible operations return `Result<T, DirSqlError>`. Statements classified as writes by SQLite's `sqlite3_stmt_readonly` surface as the unit variant `DirSqlError::WriteForbidden`; in the Python/TS bindings the same condition is a `RuntimeError` / `Error` with a "read-only" message.

### TypeScript
- Uses `camelCase` for method names.
- `RowEvent` field names use `camelCase` (`oldRow`, `filePath`), not `snake_case`.
- Table definitions are plain objects (`{ ddl, glob, extract, strict? }`), not a class.
- The constructor is overloaded: `new DirSQL(configPath: string)` or `new DirSQL(options: { root?, tables?, ignore?, config? })`. There is no separate `fromConfig` factory.
- No separate `AsyncDirSQL` — JS is async by default, so `DirSQL` has `ready: Promise<void>`, `query(): Promise<Record[]>`, and `watch(): AsyncIterable<RowEvent>` built in.
- `query()`, `startWatcher()`, and `pollEvents()` all return `Promise`s and run on the libuv threadpool so the JS event loop stays responsive (even for long poll timeouts).
- `new DirSQL(...)` returns synchronously but the initial directory scan runs on the libuv threadpool: `ready` resolves once it completes and rejects on scan error. Every method transparently awaits `ready`, so callers can issue queries immediately.

## Test Coverage Matrix

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Basic init + query         | Y      | Y    | Y          |
| Multiple tables            | Y      | Y    | Y          |
| Ignore patterns            | Y      | Y    | Y          |
| Construct from config file | Y      | Y    | Y          |
| Explicit root overrides config root | Y      | Y    | Y          |
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
| AsyncDirSQL: from config   | Y      | Y    | Y (via `new DirSQL(string)` + ready) |
| AsyncDirSQL: watch         | Y      | Y    | Y (via DirSQL.watch) |
| Persist: cold start writes cache       | Y      | Y    | Y          |
| Persist: warm start trusts cache       | Y      | Y    | Y          |
| Persist: changed file is re-parsed     | Y      | Y    | Y          |
| Persist: deleted file rows removed     | Y      | Y    | Y          |
| Persist: new file ingested             | Y      | Y    | Y          |
| Persist: racy-window triggers hash     | -      | Y    | -          |
| Persist: glob change forces rebuild    | Y      | Y    | Y          |
| Persist: dirsql_version bump rebuilds  | -      | Y    | -          |
| Persist: `.dirsql/` excluded from walk | Y      | Y    | Y          |
| Persist: custom persist_path honored   | Y      | Y    | Y          |
