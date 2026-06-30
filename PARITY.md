# SDK Parity

API surface comparison across the three language SDKs.

## Core Types

| Concept     | Python                  | Rust                     | TypeScript               |
|-------------|-------------------------|--------------------------|--------------------------|
| Table def   | `Table(ddl, glob, extract, strict)` | `Table::new(ddl, glob, extract)` / `Table::strict(...)` / `Table::try_new(...)` | `new Table({...})` or `{ ddl, glob, extract, strict? }` (plain object) |
| Extract callback | `(path) -> list[dict]` | `Fn(&str) -> Vec<Row>` | `(path) => Record<string, unknown>[]` |
| Row event   | `RowEvent` (class, frozen attrs; `file_path` on all variants) | `RowEvent` (enum: Insert/Update/Delete/Error; `file_path` on all variants) | `RowEvent` (plain object with action string; `filePath` on all variants) |
| Row type    | `dict[str, Any]`        | `HashMap<String, Value>` | `Record<string, unknown>` |

The `extract` callback receives a single argument: the absolute filesystem
path of the matched file. `dirsql` does not read file contents — a callback
that needs the file body reads it itself. This is consistent across all three
SDKs (no drift).

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
| Constructor                | `DirSQL(root=None, *, tables=None, ignore=None, config=None, persist=False, persist_path=None, extensions=None)` | `DirSQL::builder().root(..).tables(..).ignore(..).config(..).persist(..).persist_path(..).extensions(..).build()` (also `DirSQL::new`/`with_ignore` shortcuts) | `new DirSQL(configPath)` or `new DirSQL({ root?, tables?, ignore?, config?, persist?, persistPath?, extensions? })` + `await db.ready` |
| Query (read-only; rejects non-SELECT) | `db.query(sql) -> list[dict]`        | `db.query(sql) -> Result<Vec<Row>>`                  | `await db.query(sql) -> Record[]` (runs on libuv threadpool) |
| Start watcher              | `db._start_watcher()`                          | `db.start_watching()`                                | `await db.startWatcher()` (runs on libuv threadpool)    |
| Poll events                | `db._poll_events(ms)`                          | `db.poll_events(duration)`                           | `await db.pollEvents(ms)` (runs on libuv threadpool)    |
| Watch (channel/stream)     | `async for event in db.watch()` (via `_async.py`) | `db.watch() -> WatchStream` (channel)                | `for await (const ev of db.watch())`                    |
| Resolved-state serialization | _removed (#323)_ | `db.config() -> DirSQLConfig` (`serde::Serialize`)   | `db.toJSON()` / `JSON.stringify(db)` -> `DirSQLConfig`  |
| Load SQLite extension(s)   | `DirSQL(extensions=[{path, entrypoint?}])`; `[[dirsql.extension]]` config entries | `.extension(Extension)` / `.extensions(I)` builder; `[[dirsql.extension]]` config entries (`path` + optional `entrypoint`) | `new DirSQL({ extensions: [{ path, entrypoint? }] })`; `[[dirsql.extension]]` config entries |

**Extension loading — at parity across all three SDKs, see #225 / #229 / #230.**
The Rust core loads SQLite extensions declared as `[[dirsql.extension]]` config
entries or via `DirSQLBuilder::extension` / `extensions`, before any
`CREATE TABLE` (enable → load → disable). The `.dirsql.toml` form is parsed by
the shared Rust config loader. The Python `DirSQL(extensions=[{path, entrypoint?}])`
([#229](https://github.com/thekevinscott/dirsql/issues/229)) and TypeScript
`new DirSQL({ extensions: [{ path, entrypoint? }] })`
([#230](https://github.com/thekevinscott/dirsql/issues/230)) constructor
parameters marshal into that same core, and the `interpret` native-config
handshake carries an `extensions` array (`HandshakeState` / `NativeConfig`), so
a `.py` / `.js` config that declares extensions propagates. All three
resolved-state snapshots (`DirSQL::config()` / `vars(db)` / `toJSON()`) serialize
an `extensions` array (each entry `{path, entrypoint}`, empty when none
configured).

All three bindings share a single Rust implementation: `dirsql::DirSQL` handles
the initial scan, SQL, watcher, and row diffing. Python (`dirsql-py-ext`) and
TypeScript (`dirsql-napi`) bindings are thin shims that only marshal values
between the host language and Rust.

**Relative-root watching (#250) — parity maintained, no drift.** The watcher
now canonicalizes its watch-root before handing it to `notify`, so a relative
`root` emits events identically to an absolute one. The fix is entirely in the
shared Rust core (`start_watching` / `process_file_event`), so all three SDKs
gain it at once with no binding changes; the user-supplied `root` and the
`config()`/`toJSON` snapshot are unchanged across all three.

## AsyncDirSQL

| API                        | Python                                | Rust                                   |
|----------------------------|---------------------------------------|----------------------------------------|
| Constructor                | (merged into `DirSQL`; the Python `DirSQL` is already async-by-default) | `AsyncDirSQL::builder().root(..).tables(..).ignore(..).config(..).persist(..).persist_path(..).build_async()?` (also `new`/`with_ignore` shortcuts) |
| Ready                      | `await db.ready()`                    | `db.ready().await?`                    |
| Query                      | `await db.query(sql)`                 | `db.query(sql).await?`                 |
| Watch                      | `async for event in db.watch()`       | `db.watch()? -> WatchStream` (Stream trait) |

**Ready semantics — Python and TypeScript transparently await `ready`.** The
Python `DirSQL.query()` and the TypeScript `db.query()` await readiness
internally, so a query issued immediately after construction (before an
explicit `await db.ready()`) waits for the scan instead of failing. (Python
previously raised `AttributeError` in that window; now fixed to wait like
TypeScript.) Rust's `AsyncDirSQL` is intentionally more explicit: its methods
require a prior `ready().await` and return a `"not ready"` error otherwise — a
language-idiomatic difference, not unintended drift.

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

## CLI: `interpret` subcommand

The `dirsql interpret <config>` subcommand (#196) is a long-running
NDJSON helper that the Rust orchestrator spawns when `--config` points
to a native-language file. It reads `extract` requests from stdin,
dispatches to user-defined callbacks loaded from the config, and writes
results to stdout.

| SDK        | Status      | Notes                                                                    |
|------------|-------------|--------------------------------------------------------------------------|
| Python     | **Removed (#323)** | Native `.py` config + `interpret` deleted from the Python SDK (epic #321, A1). |
| TypeScript | Implemented | Loads `.js` / `.mjs` / `.cjs` config via dynamic `import()`, takes the default export. |
| Rust       | N/A         | Rust has no host language runtime in which user `extract` callbacks could execute. Intentional parity drift. |

**Root defaulting + nested-config rejection (#260) — parity maintained
(Python + TypeScript), no drift.** When a native config omits `root`, both the
Python and TypeScript `interpret` helpers default the handshake root to the
helper process's current working directory (the directory `dirsql` was invoked
from) rather than erroring; and both reject a config whose `DirSQL` itself sets
`config=` (a nested config is unrepresentable in the handshake and would
recurse), exiting non-zero with a `config=` error. Rust is `N/A` (no `interpret`
helper). As part of this the Python `DirSQL(...)` constructor no longer raises
`TypeError` when neither `root` nor `config` is given — the "no root" check is
delegated to the shared Rust core (surfacing from `ready()` / `query()`),
matching Rust and TypeScript, whose constructors already forwarded `(None, None)`
to the core.

## CLI: Native-Language Config Files

The `--config` flag accepts native-language config files in addition to
`.dirsql.toml`. The Rust binary inspects the file extension; for
non-TOML files it spawns `dirsql interpret <config>` as a subprocess
(via PATH) and wires each table's `extract` callback as an NDJSON-RPC
into that helper. The same binary handles all extensions; whether
native-language configs work depends on whether a `dirsql` launcher
that implements `interpret` is reachable on PATH.

| Install | `--config *.toml` | `--config *.py` | `--config *.{js,mjs,cjs}` |
|---|---|---|---|
| `pip install dirsql` / `uvx dirsql` | Y | **N (removed #323)** | N |
| `npm install -g dirsql` / `npx dirsql` | Y | N | Y (Node launcher handles `interpret`) |
| `cargo install dirsql --features cli` | Y | N (no launcher on PATH) | N (no launcher on PATH) |

Native-language configs are always handled by the Rust binary; the
language-specific launchers stay as thin forwarders that `exec` the
binary unchanged.

**Python config convention**: the module must define a module-level `app = DirSQL(...)`.

**JS config convention**: the module must `export default new DirSQL(...)` (ESM) or
assign to `module.exports` (CJS). Only compiled `.js` / `.mjs` / `.cjs` files are
supported; `.ts` source files are out of scope.

No MIGRATIONS.md entry is required — this is a purely additive CLI feature with no
change to the SDK's public API.

## Language-Idiomatic Exceptions

### Python
- Uses `snake_case` for all identifiers.
- `Table` is a class with keyword-only constructor args.
- `RowEvent` is a frozen class with attribute access (`event.action`, `event.row`).
- `AsyncDirSQL` is a pure-Python wrapper using `asyncio.to_thread`.
- Watch low-level methods are prefixed with `_` (private convention).
- Ships PEP 561 type information (`py.typed` + `dirsql/_dirsql.pyi`) so downstream consumers see types for `DirSQL`, `Table`, `RowEvent`. Parity-restoring: Rust types come from Rust source, TypeScript types from generated `.d.ts`, Python from the bundled stub. The stub MUST be updated in lockstep with `packages/python/src/lib.rs`.

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
- Table definitions may be written as `new Table({...})` or as a plain object literal (`{ ddl, glob, extract, strict? }`); `Table` is a thin identity wrapper that exists for parity with the Python/Rust `Table` constructors. Both forms are interchangeable at every call site that takes `TableDef[]`.
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
| Load SQLite extension(s)   | Y      | Y    | Y          |
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
| Persist: racy-window triggers hash     | Y      | Y    | Y          |
| Persist: glob change forces rebuild    | Y      | Y    | Y          |
| Persist: dirsql_version bump rebuilds  | Y      | Y    | Y          |
| Persist: `.dirsql/` excluded from walk | Y      | Y    | Y          |
| Persist: custom persist_path honored   | Y      | Y    | Y          |
