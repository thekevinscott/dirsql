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
| Load SQLite extension(s)   | `DirSQL(extensions=[{path, entrypoint?}])`; `[[dirsql.extension]]` config entries | `.extension(Extension)` / `.extensions(I)` builder; `[[dirsql.extension]]` config entries (`path` + optional `entrypoint`) | `new DirSQL({ extensions: [{ path, entrypoint? }] })`; `[[dirsql.extension]]` config entries |

**Extension loading — at parity across all three SDKs, see #225 / #229 / #230.**
The Rust core loads SQLite extensions declared as `[[dirsql.extension]]` config
entries or via `DirSQLBuilder::extension` / `extensions`, before any
`CREATE TABLE` (enable → load → disable). The `.dirsql.toml` form is parsed by
the shared Rust config loader. The Python `DirSQL(extensions=[{path, entrypoint?}])`
([#229](https://github.com/thekevinscott/dirsql/issues/229)) and TypeScript
`new DirSQL({ extensions: [{ path, entrypoint? }] })`
([#230](https://github.com/thekevinscott/dirsql/issues/230)) constructor
parameters marshal into that same core.

**Extension `path` by package name — see epic #227.** A `path` may be a bare
**package name** (no path separator and no loadable-file suffix), resolved from
the installed package in the runtime env: the SDK/launcher locates the package
dir and globs the current platform's loadable inside it (file-first probe — a
same-named local file wins; zero or multiple loadables error). This is a
per-ecosystem concern because the discovery mechanism is ecosystem-specific and
the shared Rust core stays file-path-only (epic
[#227](https://github.com/thekevinscott/dirsql/issues/227) carve-out).

| Surface | Python ([#298](https://github.com/thekevinscott/dirsql/issues/298)) | Rust | TypeScript ([#299](https://github.com/thekevinscott/dirsql/issues/299)) |
|---|---|---|---|
| Constructor `extensions` path = package name | Y (`importlib`) | N/A (file-path-only by design) | Y (`require.resolve`) |
| `.dirsql.toml` `[[dirsql.extension]]` path = package name, via SDK `config=` | Y ([#313](https://github.com/thekevinscott/dirsql/issues/313)) | N/A | Y ([#313](https://github.com/thekevinscott/dirsql/issues/313)) |
| `.dirsql.toml` `[[dirsql.extension]]` path = package name, via CLI | Y (Python launcher resolves) | N/A | Y (Node launcher resolves) |

The `.dirsql.toml` package-name form is resolved by the **SDK/launcher**, not
the compiled engine. Both the CLI launchers and (since
[#313](https://github.com/thekevinscott/dirsql/issues/313)) the SDK `config=`
construction path share one per-language helper
(`resolve_config_extension_specs` / `resolveConfigExtensionSpecs`): when a
config entry names a package, it parses the config, resolves each extension
(`importlib` / `require.resolve`), and hands the core resolved literal paths
while the core's `suppress_config_extensions` toggle stops the config's own
entries from loading too (the launchers pass the paths via `--extension`; the
SDKs via the binding's `extensions` + `suppress_config_extensions` /
`suppressConfigExtensions` parameters). Configs with only literal paths are
still loaded by the core directly. Parity restored across Python and
TypeScript; Rust stays file-path-only by design (epic #227 carve-out).

All three bindings share a single Rust implementation: `dirsql::DirSQL` handles
the initial scan, SQL, watcher, and row diffing. Python (`dirsql-py-ext`) and
TypeScript (`dirsql-napi`) bindings are thin shims that only marshal values
between the host language and Rust.

**Relative-root watching (#250) — parity maintained, no drift.** The watcher
now canonicalizes its watch-root before handing it to `notify`, so a relative
`root` emits events identically to an absolute one. The fix is entirely in the
shared Rust core (`start_watching` / `process_file_event`), so all three SDKs
gain it at once with no binding changes; the user-supplied `root` is unchanged
across all three.

**Verbatim table columns (#361, epic #358) — parity by construction, no drift.**
User tables carry exactly the columns declared in their DDL (row ownership is
tracked in the internal `_dirsql_internal_rows` table, not injected columns).
This lives entirely in the shared Rust core (`create_table` runs DDL verbatim;
`query` returns vanilla rows), so `PRAGMA table_info` and `SELECT *` report the
same user-only columns across all three SDKs.

**Internal tables unreachable through `query()` (#378, epic #358) — parity by
construction, no drift.** The internal bookkeeping tables (`_dirsql_internal_rows`,
`_dirsql_files`, `_dirsql_meta`) are denied on the `query()` path by a SQLite
authorizer in the shared Rust core (`db::query`), so a read of any `_dirsql_*`
table fails identically across all three SDKs (and the CLI's `POST /query`) with
a "not authorized" error — no per-binding surface.

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

## CLI: config files

`--config` accepts a single format across all three SDKs: `.dirsql.toml`,
parsed by the shared Rust config loader. Native-language config files (`.py` /
`.js` / `.mjs` / `.cjs`), the `dirsql interpret` NDJSON helper that backed them,
and the cross-language config-serialization snapshot (#194) that fed the
handshake were all removed in epic #321 (#323 Python, #324 TypeScript, #325
Rust + docs) — there is no parity surface here anymore. To run user-defined
`extract` callbacks, embed a binding SDK (`DirSQL(...)` + `Table(extract=fn)`)
in your own host program and query it in-process; this is at parity across
Python, Rust, and TypeScript (see *Core Types* and *DirSQL* above).

**Command-backed events (Epic B, #322) — parity by construction, no drift.**
The command runner primitive (`dirsql::command::run_command`, B1 #326) lives entirely in
the Rust core and is **not exposed on any binding's public API** — it has no
Python/TypeScript surface to keep in parity. The events built on it (`on-file`,
`pre-query`, `post-query`) are `.dirsql.toml` keys parsed by the shared Rust
config loader, so every install (`pip` / `npm` / `cargo`) gets identical
behavior with no per-SDK code. Individual event rows land here as B2–B4 ship.
All three share one global timeout override, `[dirsql].hook-timeout`
(`config::Config::hook_timeout`, positive seconds, default 30s; #351).

- **`on-file` (B2 #327).** A `[[table]]` key naming a per-file command whose
  JSON-array stdout becomes the table's rows (placeholders `{path}` / `{abspath}`
  / `{root}`; per-file error isolation; 30s default timeout, overridable via the
  global `[dirsql].hook-timeout` key in positive seconds, #351). Parsed and executed in the
  shared Rust core (`config::TableConfig::on_file` + the `build_tables_from_config`
  extract path), with **no** Python/TypeScript public-API surface — identical
  across all three installs, no drift.

- **`pre-query` (B3 #328).** A **server-wide** `[dirsql]` key naming a command
  that rewrites each `POST /query` body (passed as the `{args}` placeholder) into
  the plain-text SQL to run; failure → 500 with the stderr tail; 30s default
  timeout, overridable via the global `[dirsql].hook-timeout` (positive seconds, #351).
  Parsed by the shared Rust config loader (`config::Config::pre_query`) and
  wired through the CLI server (`cli::ServerConfig::pre_query` / the `/query`
  handler; `cli::PreQuery` carries the timeout) — a **CLI-only** surface with
  **no** Python/TypeScript public-API
  binding, identical across every install, no drift.

- **`post-query` (B4 #329).** A **server-wide** `[dirsql]` key naming a command
  that reshapes each successful `POST /query` result set (rows serialized as a
  JSON array, delivered on stdin and as the `{args}` placeholder for payloads
  ≤ 96 KiB) into the JSON response body it prints; invalid JSON or a failure
  (non-zero exit, timeout, spawn error) → 500; 30s default timeout, overridable
  via the global `[dirsql].hook-timeout` (positive seconds, #351). Parsed by the shared
  Rust config loader (`config::Config::post_query`) and
  wired through the CLI
  server (`cli::ServerConfig::post_query` / the `/query` handler;
  `cli::PostQuery` carries the timeout) — a
  **CLI-only** surface with **no** Python/TypeScript public-API binding,
  identical across every install, no drift.

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

Feature × SDK × tier coverage parity (#294). Unless a cell says otherwise,
`Y` means the scenario is covered in that SDK's **real-core tier**: for
Python/TypeScript the **binding** tier (`tests/binding/`, SDK public API
against the real core + real temp dirs, #289), for Rust the integration
tier (`packages/rust/tests/` — Rust *is* the core, so it has no binding
tier). `integration` means the Python/TS **hermetic integration** tier
(`tests/integration/`, SDK public API with the core and filesystem mocked,
#289). `core` means the behavior lives in the shared Rust core and is
deliberately covered once, at the Rust integration/unit tier — per the
one-implementation principle, the bindings prove marshaling, not the core
logic itself. `unit` means the SDK covers it at its colocated-unit tier
(idiomatic for Rust inline `#[cfg(test)]` modules). `N/A` means the surface
does not exist in that SDK by design (see Language-Idiomatic Exceptions).

Real-core file map:

| Area | Python (`packages/python/tests/binding/`) | Rust (`packages/rust/tests/`) | TypeScript (`packages/ts/tests/binding/`) |
|---|---|---|---|
| Core SDK | `dirsql_test.py` | `sdk.rs` | `index.test.ts`, `docs-gaps.test.ts` |
| Async / ready | `async_dirsql_test.py` | `async_sdk.rs` | `index.test.ts` |
| Watch events | `async_dirsql_test.py`, `docs_gaps_test.py` | `sdk.rs`, `watcher.rs`, `watch_relative_root.rs` | `watch.test.ts`, `index.test.ts` |
| Config file | `from_config_test.py` | `from_config.rs`, `config.rs` | `from-config.test.ts` |
| Persistence | `persist_test.py` | `persist.rs` | `persist.test.ts` |
| Extensions | `extensions_test.py`, `extension_package_test.py`, `config_extension_package_test.py` | `extensions.rs` | `extensions.test.ts`, `extension-package.test.ts`, `config-extension-package.test.ts` |
| Table-name resolution (#204) | `table_name_resolution_test.py` | `table_name_resolution.rs` | `table-name-resolution.test.ts` |
| Docs examples | `docs_examples_test.py` | `docs_examples.rs` | `docs-examples.test.ts` |
| Docs gap-fills | `docs_gaps_test.py` | `docs_gaps.rs` | `docs-gaps.test.ts` |

Hermetic integration tier (mocked core + fs, both bindings, #289): Python
`tests/integration/dirsql_test.py` (ready/query/watch/kwarg forwarding) and
`tests/integration/extensions_test.py` (extension-path resolution, incl. the
#313 config-entry resolution + suppress toggle); TypeScript
`tests/integration/index.test.ts` (constructor overloads, positional
marshaling, delegation, watch) and `tests/integration/extensions.test.ts`
(extension-path resolution, incl. #313).

### Construction & querying

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Basic init + query         | Y      | Y    | Y          |
| Multiple tables            | Y      | Y    | Y          |
| JOIN across tables         | Y      | Y    | Y          |
| Ignore patterns            | Y      | Y    | Y          |
| Extract receives the matched file's path | Y | Y | Y |
| Extract returns `[]` to skip a file | Y | Y | Y |
| Value types (str/int/float/bool/None) | Y | Y | Y |
| `bytes`/`Vec<u8>`/`Buffer` → BLOB round-trip | Y | Y | Y (#343: `Buffer`/`Uint8Array` in, `Buffer` out) |
| Invalid SQL raises         | Y      | Y    | Y          |
| Invalid DDL raises         | Y      | Y    | Y          |
| Query rejects writes       | Y      | Y    | Y          |
| Write-rejection edge matrix (leading comments, mixed case, CTE/whitespace allowed) | core | Y (`readonly_query.rs`) | core |
| Internal `_dirsql_*` columns hidden from `SELECT *` | Y | Y | Y |
| `_dirsql_*` filter robustness (comment/string-literal bypass) | core | Y (`code_review_findings.rs`) | core |
| Empty directory / empty result set | Y | Y | Y |
| Error taxonomy (duplicate table, invalid glob, unparseable DDL, extract error) | core | Y (`sdk.rs`) | core |
| Relaxed schema (extra keys)| Y      | Y    | Y          |
| Relaxed schema (missing)   | Y      | Y    | Y          |
| Strict mode (extra keys)   | Y      | Y    | Y          |
| Strict mode (missing keys) | Y      | Y    | Y          |
| Strict mode (exact match)  | Y      | Y    | Y          |
| `Table` construction + `ddl`/`glob` attributes | Y | Y | Y (`Table` class + plain-object interchangeability) |
| Quoted-identifier DDL registers/queries by bare name (#204) | Y | Y | Y |

### Ready / async semantics

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Constructor returns before the scan completes | Y | Y (`async_sdk.rs`) | Y (#146) |
| AsyncDirSQL: ready + query | Y      | Y    | Y (via DirSQL.ready) |
| AsyncDirSQL: multiple ready| Y      | Y    | Y (via DirSQL.ready) |
| AsyncDirSQL: from config   | Y      | Y    | Y (via `new DirSQL(string)` + ready) |
| AsyncDirSQL: watch         | Y      | Y    | Y (via DirSQL.watch) |
| ready re-raises scan/init errors | Y | Y | Y (`ready` rejection) |
| Query issued eagerly awaits ready transparently | Y | N/A — explicit `ready().await` by design | Y (#146) |
| Methods before ready error (`not ready`) | N/A | Y (`async_sdk.rs`) | N/A |
| Event-loop / host-thread non-blocking | integration (`dirsql_test.py` offload seams) | N/A | Y (#146/#147 libuv-threadpool tests) |

### Watching

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Watch: insert (via async iterator/stream) | Y | Y | Y |
| Watch: delete              | Y      | Y    | Y          |
| Watch: update              | Y      | Y    | Y          |
| Watch: error               | Y      | Y    | Y          |
| Error events carry table attribution | Y | Y | Y |
| DB kept in sync after events | Y | Y | Y |
| `file_path`/`filePath` is relative to root | Y | Y | Y |
| Shrinking file ends with dropped row deleted | Y | unit (`differ.rs`) | Y |
| Low-level start-watcher + poll primitives | integration (`dirsql_test.py`) | Y (`sdk.rs`) | Y (`index.test.ts`) |
| Relative-root watching (#250) | core | Y (`watch_relative_root.rs`) | core |
| watch/poll mutual-exclusion errors | core | Y (`sdk.rs`) | core |

### Config file (`.dirsql.toml` via the SDK)

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Construct from config file | Y      | Y    | Y          |
| Explicit root overrides config root | Y      | Y    | Y          |
| One row per matched file + stat columns (`path`, `basename`, `dir`, `ext`, `size`, `mtime`) | Y | Y | Y |
| Glob path captures promoted to columns | Y | Y | Y |
| Config `[dirsql].ignore` respected | Y | Y | Y |
| Multiple `[[table]]` entries | Y | Y | Y |
| Missing config file errors | Y | Y | Y |
| Invalid TOML errors        | Y | core | Y |
| `[[table]]` missing `ddl` errors | Y | core | Y |
| Config `persist` / `persist_path` resolution | core | Y (`from_config.rs`) | core |

### Persistence

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Persist: cold start writes cache       | Y      | Y    | Y          |
| Persist: warm start trusts cache       | Y      | Y    | Y          |
| Persist: changed file is re-parsed     | Y      | Y    | Y          |
| Persist: deleted file rows removed     | Y      | Y    | Y          |
| Persist: new file ingested             | Y      | Y    | Y          |
| Persist: racy-window triggers hash     | Y      | unit (`lib.rs` reconcile tests) | Y |
| Persist: glob change forces rebuild    | Y      | Y    | Y          |
| Persist: dirsql_version bump rebuilds  | Y      | Y    | Y          |
| Persist: `.dirsql/` excluded from walk | Y      | Y    | Y          |
| Persist: custom persist_path honored   | Y      | Y    | Y          |

### Extensions

| Test Scenario              | Python | Rust | TypeScript |
|----------------------------|--------|------|------------|
| Load SQLite extension(s)   | Y      | Y    | Y          |
| Missing constructor extension fails ready/build | Y | Y | Y |
| Optional `entrypoint` carried into the load call | Y | Y | Y |
| No extensions → normal build | Y | Y | Y |
| Missing `[[dirsql.extension]]` config entry fails ready/build | Y | Y | Y |
| Real extension loaded + function callable (fixture cdylib) | Y (`extension_package_test.py`) | Y (`extensions.rs`) | Y (`extension-package.test.ts`) |
| `path` as bare package name (constructor, #298/#299) | Y | N/A — file-path-only by design | Y |
| Config `[[dirsql.extension]]` `path` as bare package name via SDK `config=` (#313) | Y + integration | N/A | Y + integration |
| `load_extension()` locked after startup; `suppress_config_extensions` seam | core | Y (`extensions.rs`) | core |

### E2E (CLI / launcher) and smoke tiers

The CLI is a single Rust binary shipped through three channels, so its
*behavior* (HTTP `/query` + `/events`, status codes, zero-config `files`
table, `init`, `on-file` / `pre-query` / `post-query` hooks, signal
handling) is covered once, in the Rust e2e/CLI suites (`cli_e2e.rs`,
`cli_integration.rs`, `init_e2e.rs`, `init_integration.rs`,
`on_file_e2e.rs`). The per-binding e2e suites cover what is genuinely
per-launcher: resolving/staging the bundled binary, forwarding argv, and
ecosystem-specific extension resolution.

| Test Scenario              | Python (`tests/e2e/`, `tests/smoke/`) | Rust (`tests/`) | TypeScript (`tests/e2e/`, `tests/smoke/`) |
|----------------------------|--------|------|------------|
| `--version` exits 0 and prints the version | Y (`cli_version_test.py`) | Y (`cli_e2e.rs`) | Y (smoke `build.test.ts`, against the packed npm install) |
| Launcher starts server; `POST /query` over HTTP | Y (`extension_package_test.py`) | Y | Y (`extension-package.test.ts`) |
| `[[dirsql.extension]]` package name resolved by the launcher (#227) | Y | N/A | Y |
| `interpret` subcommand removed; argv forwarded to clap (#321) | Y | core (clap dispatch) | Y |
| HTTP semantics, SSE `/events`, hooks, `init`, zero-config `files` table | core | Y | core |
| Smoke: pack → install → run the published artifact | Y (smoke `build_test.py`, against the packed wheel install) | N/A | Y |

### Known gaps / follow-ups

- **#289** — resolved: the integration tier is hermetic in both bindings
  (Python patches `_RustDirSQL` via `unittest.mock`; TypeScript delivers a
  fake core module through a mocked `node:module` `createRequire`), and the
  former real-core integration suites moved to the per-binding
  `tests/binding/` tier, which still runs in CI.
