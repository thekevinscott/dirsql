# SDK Parity

API surface comparison across the three language SDKs.

## Core Types

| Concept     | Python                  | Rust                     | TypeScript               |
|-------------|-------------------------|--------------------------|--------------------------|
| Table def   | `Table(ddl, glob, extract, strict)` | `Table::new(ddl, glob, extract)` / `Table::strict(...)` / `Table::try_new(...)` | `{ ddl, glob, extract, strict? }` (plain object) |
| Row event   | `RowEvent` (class, frozen attrs) | `RowEvent` (enum: Insert/Update/Delete/Error) | `RowEvent` (plain object with action string) |
| Row type    | `dict[str, Any]`        | `HashMap<String, Value>` | `Record<string, unknown>` |

## DirSQL (synchronous)

| API                        | Python                           | Rust                              | TypeScript                        |
|----------------------------|----------------------------------|-----------------------------------|-----------------------------------|
| Constructor                | `DirSQL(root, *, tables, ignore)` | `DirSQL::new(root, tables)` / `DirSQL::with_ignore(root, tables, ignore)` | `new DirSQL(root, tables, ignore?)` |
| From config                | `DirSQL.from_config(path)`       | `DirSQL::from_config(root_dir)`   | `DirSQL.fromConfig(configPath)`   |
| Query                      | `db.query(sql) -> list[dict]`    | `db.query(sql) -> Result<Vec<Row>>` | `db.query(sql) -> Record[]`     |
| Watch (low-level)          | `db._start_watcher()` / `db._poll_events(ms)` | `db.watch() -> WatchStream` (channel) | `db.startWatcher()` / `db.pollEvents(ms)` |

## AsyncDirSQL

| API                        | Python                                | Rust                                   | TypeScript                              |
|----------------------------|---------------------------------------|----------------------------------------|-----------------------------------------|
| Constructor                | `AsyncDirSQL(root, *, tables, ignore)` | `AsyncDirSQL::new(root, tables)?` / `with_ignore(...)` | `new AsyncDirSQL(root, tables, ignore?)` |
| From config                | `AsyncDirSQL.from_config(path)`       | `AsyncDirSQL::from_config(root_dir)?`  | `AsyncDirSQL.fromConfig(configPath)`    |
| Ready                      | `await db.ready()`                    | `db.ready().await?`                    | `await db.ready()`                      |
| Query                      | `await db.query(sql)`                 | `db.query(sql).await?`                 | `await db.query(sql)`                   |
| Watch                      | `async for event in db.watch()`       | `db.watch()? -> WatchStream` (Stream trait) | `for await (const event of db.watch())` |

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
- `RowEvent` is a Rust enum with variants (`Insert { table, row }`, etc.) rather than a flat struct.
- `DirSQL::from_config` takes a root directory path (looks for `.dirsql.toml` inside), not the config file path directly.
- `AsyncDirSQL` uses tokio and `OnceCell` internally.
- Watch returns `futures_channel::mpsc::UnboundedReceiver<RowEvent>` implementing `Stream`.
- All fallible operations return `Result<T, DirSqlError>`.

### TypeScript
- Uses `camelCase` for method names (`fromConfig`, `startWatcher`, `pollEvents`).
- `RowEvent` field names use `camelCase` (`oldRow`, `filePath`), not `snake_case`.
- Table definitions are plain objects (`{ ddl, glob, extract, strict? }`), not a class.
- `DirSQL.fromConfig` takes the config file path directly (like Python), not the root directory (like Rust).
- `AsyncDirSQL` is a pure-JS wrapper class (not native).
- Watch returns an `AsyncIterable<RowEvent>`.

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
| Relaxed schema (extra keys)| Y      | Y    | Y          |
| Relaxed schema (missing)   | Y      | Y    | Y          |
| Strict mode (extra keys)   | Y      | Y    | Y          |
| Strict mode (missing keys) | Y      | Y    | Y          |
| Strict mode (exact match)  | Y      | Y    | Y          |
| AsyncDirSQL: ready + query | Y      | Y    | Y          |
| AsyncDirSQL: multiple ready| Y      | Y    | Y          |
| AsyncDirSQL: from_config   | Y      | Y    | Y          |
| AsyncDirSQL: watch         | Y      | Y    | Y          |
