# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Security

- **`query()` now rejects `ATTACH`/`DETACH` (#462, epic #461).** SQLite classifies `ATTACH` as read-only, so it slipped past the read-only gate on `query()` — a caller reaching the surface (SDK `query`, CLI `POST /query`, `dirsql query`) could run `ATTACH '/path/x.db' AS ext` to create an arbitrary file on disk and then read an external database via `SELECT ... FROM ext.*`. The query-path authorizer now denies both `ATTACH` and `DETACH` at prepare time, surfaced as the same not-authorized error the `_dirsql_*` denial uses, so neither ever executes and no file is created. All other effectful statements were already blocked as writes; `ATTACH`/`DETACH` were the only read-only-classified actions that leaked. See `MIGRATIONS.md`.

### Fixed

- **TypeScript SDK surfaces native-load and construction errors instead of masking them (#467, epic #461).** The native-addon loader no longer swallows every error from the platform `@dirsql/lib-*` sub-package: only a genuine `MODULE_NOT_FOUND` falls through to the dev-path binary, while any other loader failure (ABI/glibc mismatch, corrupt binary) now propagates verbatim rather than being replaced by a misleading "Cannot find module .../dirsql.node". The `DirSQL` constructor also attaches a no-op handler to its internal readiness promise, so constructing without awaiting `ready` (as the docs encourage) can no longer terminate the process with an unhandled rejection when construction fails — the real error surfaces at the first `query()` / `await db.ready` instead.
- **Reserved-word column names are now writable (#463).** `insert_row` interpolated user column names unquoted, so a table legally declaring a reserved word as a column (e.g. `"order" INTEGER`) passed validation but failed every insert with `near "order": syntax error`, leaving the table permanently un-writable. Column identifiers (and the table name) are now quoted on insert, matching the read path (`get_rows_by_file`); `validate_identifier` remains the injection guard.

### Changed

- **Binding-boundary value fidelity: out-of-range integers now error, and extract errors carry their real message (#465, epic #461).** A symmetric numeric contract across both bindings: an integer that does not fit a signed 64-bit `Value::Integer` is a hard error, never a lossy conversion. Python raises `OverflowError` for an `int` beyond `i64` (previously it silently degraded to a lossy `REAL` via `__float__`, or a `TEXT` repr); the TypeScript SDK throws when a query result exceeds `Number.MAX_SAFE_INTEGER` (2^53 − 1) instead of returning a rounded `number`, and a JS `bigint` outside `i64` throws on extract. A JS `bigint` **within** `i64` range now maps to `INTEGER` (previously stored as `TEXT`). Python `py_to_value` no longer probes a `list`/`tuple` of small ints as bytes — only a real `bytes`/`bytearray` maps to `BLOB`, so `[1,2,3]` and `[1,2,300]` behave identically (both `TEXT`). The napi extract path now propagates the real thrown JS exception message (e.g. `throw new Error("bad JSON …")`) instead of the fixed `"Extract function call failed"`, matching the pyo3 side. See `MIGRATIONS.md`.
- **Breaking: the seven stat columns dropped their leading underscore (#454, epic #452).** `_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime` are now `path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`. These were never an enforced/reserved namespace — the underscore prefix was purely conventional — so this is a pure rename with no other behavior change. Existing `.dirsql.toml` configs and SDK table definitions declaring the old names need updating; see `MIGRATIONS.md`.
- **Breaking: `dirsql init` is now deterministic (#455, epic #452).** It no longer shells out to the `claude` CLI. It writes a fixed starter `.dirsql.toml` — the exact single `files` table zero-config mode already serves — parsed from one embedded default-config asset shared by both surfaces, so they can never drift apart. `init` no longer inspects the target directory at all; `--root` now only controls where the default `--output` path is resolved. No LLM, no network, no `claude` dependency. See `MIGRATIONS.md`.
- **Documentation: trimmed LLM-drafted comments to the minimal useful set (#445).** Removed archaeology comments (issue/PR references), comments restating adjacent code, reviewer-directed justification, and banner dividers across all three SDKs. Retained public API docs, safety/locking/ordering invariants, platform quirks, security invariants, and forward-looking notes tied to open issues. No code behavior changes.

### Added

- **One-shot `dirsql query "<sql>"` subcommand (#399/#439).** Build the index,
  run one query, print the result rows as a JSON array on stdout, and exit —
  no server, no watch, pipes straight into `jq`. The subcommand is a thin
  adapter over the exact pipeline `POST /query` uses (extracted in #438), so
  config discovery (`--config`, zero-config `files` table, `--extension`
  overrides), the `pre-query`/`post-query` hooks, the 30-second query timeout,
  the read-only rule, and the `_dirsql_*` internal-table denial (#378) behave
  identically on both surfaces by construction. Errors print the same
  diagnostic the HTTP `{"error": …}` body carries, on stderr, with a non-zero
  exit.

- **Configurable command-hook timeout (#351).** A single global
  `[dirsql].hook-timeout` key (positive whole seconds) raises or tightens the
  30-second timeout for **all** command-backed hooks at once — `on-file`,
  `pre-query`, and `post-query` — so an embeddings extractor that downloads a
  model on first run, or a slow (e.g. LLM-backed) query translator, no longer
  silently hits the fixed 30s bound. Zero and negative values are rejected with
  a config error naming the field. The default is unchanged at 30 seconds —
  existing configs behave identically. Parsed by the shared Rust config loader,
  so every install (pip/npm/cargo) gets identical behavior with no per-SDK code;
  the Rust CLI server API grows `PreQuery::with_timeout` /
  `PostQuery::with_timeout` (and a public `timeout` field on both) to carry the
  configured value.
- **Rust core: `combine_configs` merges multiple TOML configs (#352).** A pure,
  order-significant merge function in the core's `config` module — substrate
  for the plugin model explored in #341, where plugin TOML fragments merge
  additively into the project config. Each input carries a `Source` label (a
  config file path or a plugin package name) so conflict errors can name both
  sides. List-shaped config (`[[table]]`, `[[dirsql.extension]]`, `ignore`)
  concatenates in input order; a table-name collision across configs errors
  naming both sources; single-valued keys (`root`, `persist`, `persist_path`,
  `pre-query`, `post-query`) defined by more than one config error naming both
  sources — no silent shadowing, no precedence — and merge through unchanged
  when defined in exactly one. A single entry returns unchanged; an empty slice
  is rejected. Implemented once in the shared core per the one-implementation
  principle; no binding surface yet, and existing single-config loads are
  unaffected.

- **Internal row-bookkeeping table `_dirsql_internal_rows` (#359, epic #358,
  stage 1).** The engine now maintains an internal mapping table —
  `(table_name, file_path, row_index, rowid_ref)` — that mirrors the injected
  `_dirsql_file_path` / `_dirsql_row_index` tracking columns, dual-written in
  the same SQLite transaction as each row insert/delete so it can never diverge
  from the rows it describes. This is foundational plumbing for eventually
  dropping the injected columns; the injected columns remain authoritative and
  **there is no user-visible change**. `WITHOUT ROWID` tables emit a stderr
  warning (they
  break rowid-based bookkeeping and will be rejected in a later stage). The
  persistent-cache schema version is bumped, so the first startup after
  upgrading performs a one-time, penalty-free full rebuild to populate the
  mapping.

- **TypeScript SDK: `Buffer`/`Uint8Array` → SQLite BLOB (#343).** An `extract`
  callback can now return a `Buffer` or `Uint8Array` and it is stored as a real
  BLOB, restoring parity with Python's documented `bytes → BLOB` mapping.
  Previously the value was silently coerced to its string representation
  (`"0,1,2,…"`) before it reached the database.

- **Python/TypeScript SDKs: config-file `[[dirsql.extension]]` entries may name
  an extension by package name (#313, epic #227).** Constructing a `DirSQL`
  from a `.dirsql.toml` (`DirSQL(config=...)` / `new DirSQL(configPath)`) now
  resolves a `[[dirsql.extension]]` `path` that is a bare **package name** from
  the installed package in the runtime env — previously only the programmatic
  `extensions` argument (#298/#299) and the CLI launchers could do this, and
  the SDK `config=` form was literal-path-only. When any config entry names a
  package, the SDK resolves *every* entry itself (via the shared
  `resolve_config_extension_specs` / `resolveConfigExtensionSpecs` helper, also
  now backing the CLI launchers), appends the resolved literal paths after the
  programmatic extensions, and suppresses the core's own config-extension
  loading through a new `suppress_config_extensions` /
  `suppressConfigExtensions` binding parameter (wired to the existing
  `DirSQLBuilder::suppress_config_extensions` toggle) so entries are not loaded
  twice. Configs with only literal paths keep the core's existing loading and
  error reporting untouched. The Rust core and SDK remain file-path-only by
  design (epic #227 carve-out).
- **CLI server: a server-wide `post-query` command hook (B4 of epic #322,
  #329).** Set `[dirsql].post-query = "…"` in `.dirsql.toml` and the HTTP server
  reshapes every successful `POST /query` response through it: the result rows
  are serialized to a JSON array and handed to the command on stdin (always,
  unbounded and injection-safe) and as the `{args}` placeholder (for payloads
  ≤ 96 KiB; beyond that `{args}` is emptied with a stderr warning and stdin
  carries the full set — not truncation). The JSON body the command prints on
  stdout (last non-empty line) becomes the `200 application/json` response, so
  clients can receive an envelope, projected fields, or any shape — the
  canonical example is `jq -c '{results: .}'` (compact, since the body is the
  command's last stdout line). Output that isn't valid JSON returns
  `500 post-query did not return valid JSON: …`; a failure (non-zero exit,
  timeout, spawn error) returns `500` with the command's stderr tail. The hook
  runs in the config file's directory with the inherited environment and a fixed
  30-second timeout. When `post-query` is absent the rows are returned as-is
  (fully backward compatible). Handled entirely in the shared Rust core, so
  every install behaves identically with no per-SDK surface.
- **CLI server: a server-wide `pre-query` command hook (B3 of epic #322,
  #328).** Set `[dirsql].pre-query = "…"` in `.dirsql.toml` and the HTTP server
  routes every `POST /query` through it: the raw request body is passed to the
  command as the injection-safe `{args}` placeholder, and the plain-text SQL the
  command prints on stdout (last non-empty line) is what runs — so clients can
  post natural language, a saved-query name, or any DSL and have the hook
  translate it to SQL. The hook runs in the config file's directory with the
  inherited environment and a fixed 30-second timeout; a failure (non-zero exit,
  timeout, or spawn error) returns `500` with the command's stderr tail. When
  `pre-query` is absent the body is still parsed as `{"sql": …}` (fully backward
  compatible). Because the hook returns plain SQL, it is the trusted component
  that turns the untrusted body into safe SQL — intentional for v1. Handled
  entirely in the shared Rust core, so every install behaves identically with no
  per-SDK surface.

### Fixed

- **Python `DirSQL.watch()` now awaits readiness before starting the watcher
  (#464, epic #461).** `watch()` previously captured the core handle at call
  time; when called before the background scan finished (i.e. before an explicit
  `await db.ready()`), it captured a still-`None` handle permanently and the
  first iteration raised `AttributeError: 'NoneType' object has no attribute
  '_start_watcher'`, breaking the stream for good — and a failed init surfaced as
  that same `AttributeError` rather than the real error. The returned stream now
  awaits `ready()` and re-reads the core handle on its first iteration, mirroring
  `DirSQL.query()` and the TypeScript SDK's `watch()`. Restores cross-SDK parity.

### Changed

- **The default (non-persist) index is now an anonymous disk-backed temp
  database, never `:memory:` (#402, epic #400).** With `persist = false` the
  engine opens SQLite's anonymous temp database (`Connection::open("")`)
  instead of an in-memory one: SQLite creates a private temp file, deletes it
  immediately (so the OS reclaims it even on SIGKILL — nothing to clean up,
  no name to collide on), and spills index pages to disk as the index grows.
  Resident memory for large corpora drops from O(indexed data) to roughly the
  SQLite page cache. The API, query results, watch events, and persistence
  semantics are unchanged; the file lands in the directory SQLite's VFS picks
  (`SQLITE_TMPDIR` → `TMPDIR` → `/var/tmp` → `/usr/tmp` → `/tmp`) — export
  `SQLITE_TMPDIR` to steer it off a tmpfs mount. See `MIGRATIONS.md`.

- **The engine no longer keeps a full in-memory copy of every extracted row
  (#401, epic #400).** The watcher's diffing path previously retained all rows
  of every indexed file in native memory for the lifetime of the instance —
  resident memory scaled with the total indexed row data, on top of SQLite
  itself. File-change events now snapshot a file's previous rows back out of
  SQLite (via `_dirsql_internal_rows`, ordered by row index) at event time, so
  steady-state memory no longer grows with corpus size and the diffing working
  set is one file's rows. `RowEvent` semantics are unchanged; the one nuance is
  that `Update`/`Delete` old-row payloads now reflect the values as SQLite
  stored them, so an extract whose value types disagree with the declared
  column affinities (e.g. `Integer` into a TEXT column) sees the post-coercion
  value — the same value `query()` returns — instead of the pre-coercion one.

- **Internal bookkeeping tables are now unreachable through `query()` (#378,
  epic #358).** dirsql's internal tables — `_dirsql_internal_rows`,
  `_dirsql_files`, `_dirsql_meta` — were private only by the `_dirsql_` naming
  convention; the engine now enforces it. A SQLite authorizer installed on the
  `query()` path denies any read (or schema `PRAGMA`) targeting the reserved
  `_dirsql_*` namespace, so a `SELECT` / `PRAGMA` against an internal table
  fails at prepare time with a clear "not authorized" error (surfaced as HTTP
  `400` on the CLI's `POST /query`) instead of leaking the rows. The authorizer
  scopes to the user-facing query surface only — the engine still writes the
  internal tables in the same transaction as the user rows, so crash-atomicity
  is preserved and normal user queries are unaffected. Enforced once in the
  shared Rust core, so every SDK and the CLI behave identically.

- **Internal row ownership is now read from `_dirsql_internal_rows` (#360, epic
  #358, stage 2).** The engine's row readers — delete-by-file and the warm-start
  row rebuild — now resolve which rows belong to a file through the internal
  mapping table (joined on `rowid`) instead of the injected `_dirsql_file_path`
  / `_dirsql_row_index` columns, which become **write-only**. Behavior is
  unchanged and there is no user-visible difference; this is the second step of
  removing the injected columns (stage 3 stops writing them and deletes the
  `SELECT *` laundering layer). No cache rebuild is needed — the mapping was
  already populated in stage 1.

- **TypeScript SDK: BLOB columns now come back as `Buffer` (#343).** `query()`
  results and watcher `RowEvent` rows return BLOB values as Node `Buffer`s
  instead of lowercase hex strings. The CLI's HTTP/JSON surface is unchanged
  (JSON cannot carry binary; blobs stay hex-encoded there). See
  `MIGRATIONS.md` for the upgrade note.

### Removed

- **Injected `_dirsql_file_path` / `_dirsql_row_index` tracking columns (#361,
  epic #358).** dirsql no longer rewrites user DDL to add tracking columns —
  `create_table` runs the DDL verbatim, so `PRAGMA table_info` and `SELECT *`
  return exactly the columns the user declared, and query results are vanilla
  SQLite. Row ownership now lives entirely in the internal `_dirsql_internal_rows`
  table, completing the stage 1–3 migration. Documented usage is unaffected:
  `SELECT *` already excluded these columns, so its results are unchanged. The
  only observable differences are that explicitly naming the (always-undocumented)
  `_dirsql_*` columns in a query now returns a "no such column" error — use the
  documented `_path` and friends instead — and the persistent-cache schema version
  is bumped, so the first startup after upgrading performs one automatic,
  penalty-free rebuild. See `MIGRATIONS.md`.

- **Python SDK: native-language (`.py`) config support and the `dirsql
  interpret` subcommand — hard removal, no deprecation window (A1 of epic
  #321, #323).** The Python launcher no longer intercepts `interpret`; the
  `dirsql/cli/interpret/` package (the NDJSON `run` loop, `load_app`,
  `dispatch_extract`, `write_message`) is deleted, and `dirsql interpret …`
  now exits non-zero (the launcher forwards it to the binary, which rejects
  the unknown subcommand). The Python side of the cross-language
  config-serialization snapshot (#194) is retired with it: `DirSQL.__dict__`
  / `vars(db)` and the `resolve_config` helper are removed. The
  **programmatic SDK** — `DirSQL(...)` with in-process `Table(extract=fn)`
  closures — is unaffected, and `DirSQL(config="…toml")` still loads TOML via
  the core. TypeScript (#324) and Rust + docs (#325) are removed in the
  follow-up PRs of this epic.

- **TypeScript SDK: native-language (`.js` / `.mjs` / `.cjs`) config support
  and the `interpret` CLI dispatch — hard removal, no deprecation window (A2
  of epic #321, #324).** The launcher (`cli/main.ts`) no longer dispatches
  `argv[0] === "interpret"`; the `src/cli/interpret/` modules (the NDJSON
  `interpret` loop, `load-app`, `dispatch-extract`, `build-tables`,
  `err-message`, `write-message`) are deleted, and `dirsql interpret …` now
  exits non-zero (the launcher forwards it to the binary, which rejects the
  unknown subcommand). The TypeScript side of the cross-language
  config-serialization snapshot (#194) is retired with it: `DirSQL.toJSON()` /
  `JSON.stringify(db)` and the `resolveConfig` helper are removed, along with
  the now-unused `DirSQLConfig` / `TableConfig` / `ResolvedExtension` exported
  types (the unused `smol-toml` dependency is dropped). The **programmatic
  SDK** — `new DirSQL(...)` with in-process `extract` closures — is
  unaffected, and `new DirSQL("…toml")` still loads TOML via the core. Rust +
  docs (#325) follow. (#324)

- **Rust core / CLI binary: native-language config orchestration, the
  cross-language config-serialization snapshot (#194), and the native-config
  docs — hard removal (A3 of epic #321, #325; lands last).** The `dirsql`
  binary no longer inspects the `--config` extension or spawns a `dirsql
  interpret` helper; `cli::native_config` (the NDJSON spawn/handshake
  protocol) is deleted, so a non-TOML `--config` now fails to parse as TOML
  and the server starts degraded (HTTP 503). The `DirSQL::config()` snapshot
  and its `DirSQLConfig` type are removed — nothing consumed the serialized
  state once `interpret` was gone (retires #194). `.dirsql.toml` is the only
  config format the CLI accepts (unchanged for TOML users). The
  **native-language config documentation** (the *Native-Language Configs*
  sections of `docs/cli/config.md`, and the `.py`/`.js` references in
  `docs/cli/server.md` / `docs/cli/index.md`) is removed with the feature
  (docs-as-spec). Completes the epic across all three SDKs. (#325)

### Fixed

- **`POST /query` returns 400, not 500, for a rejected write statement (#444).**
  `classify_query_error` classified only `DirSqlError::Core(_)` as the
  caller's fault, so the read-only rejection (`DirSqlError::WriteForbidden`)
  fell to the server-fault catch-all and surfaced as HTTP 500 — contradicting
  `docs/reference/http-api.md`, which documents the read-only rule under 400.
  `WriteForbidden` now maps to the same 400 class as a `Core` SQL error. Applies
  identically to the HTTP server and the `dirsql query` subcommand, since both
  share the `execute_query` pipeline.

- **npm package ships the current docs tree (#404).** The `prepack` docs
  stager (`packages/ts/tools/stage-docs.ts`) still allow-listed the pre-Diataxis
  `guide/` and `api/` directories, which #403 deleted — so the published npm
  tarball shipped none of the actual documentation directories. It now stages
  the live `howto/` and `reference/` trees plus the root `explanation.md`.
  Also repoints stale `docs/guide/`, `docs/cli/`, and `docs/api/` citations that
  lingered in `packages/*` source comments and tests to their Diataxis
  successors, and fixes the dead `docs/cli/config.md#loading-extensions` /
  `github.io/dirsql/cli/` links in the three SDK READMEs.

### Added

- **Python SDK: resolve a constructor extension by package name (#298).** An
  `extensions=[{ "path": ... }]` entry whose `path` is a bare **package name**
  (no path separator and no loadable-file suffix) is resolved from the package
  installed in the runtime env: dirsql locates it via `importlib` and globs the
  current platform's loadable file (`*.so` / `*.dylib` / `*.dll` / `*.pyd`)
  inside it. A same-named local file takes precedence (file-first probe); zero
  or multiple matching loadables is an error. Path-looking values keep their
  #229 behavior unchanged. Resolution runs in the SDK before the file-path-only
  Rust core. (#298, part of #227)

- **CLI: resolve a `.dirsql.toml` `[[dirsql.extension]]` by package name (#227).**
  Running `dirsql --config .dirsql.toml` through the Python launcher now resolves
  an extension whose `path` is a bare package name from the installed package,
  before invoking the engine. The compiled binary can't resolve package names
  (no `importlib`), so the launcher parses the config, resolves each extension,
  and passes the resolved literal paths to the binary through a new repeatable
  `--extension <path>[::entrypoint]` flag; the binary loads those and skips the
  config's own `[[dirsql.extension]]` entries. The Rust core gains a
  `DirSQLBuilder::suppress_config_extensions(bool)` toggle backing this (the core
  stays file-path-only). Configs with only literal paths are untouched. (Node
  launcher parity tracked alongside; #227.)

- **TypeScript SDK: resolve an extension by package name — constructor + TOML
  CLI (#299).** Restores parity with the Python sibling (#298). A
  `extensions: [{ path }]` entry (or a `.dirsql.toml` `[[dirsql.extension]]`
  run through the Node launcher) whose `path` is a bare **package name** is
  resolved from the package installed under `node_modules`: dirsql locates it
  via `require.resolve` and globs the current platform's loadable file
  (`*.so` / `*.dylib` / `*.dll` / `*.node`) inside it. A same-named local file
  takes precedence (file-first probe); zero or multiple matching loadables is
  an error. Path-looking values keep their #230 behavior unchanged.
  Constructor resolution runs in the SDK before the file-path-only Rust core;
  the CLI form is resolved by the Node launcher, which passes the resolved
  literal paths to the binary via `--extension` (as the Python launcher does).
  (#299, part of #227)

- **`.dirsql.toml`: `on-file` per-table command event — the first
  command-backed event (Epic B, #322 / B2 #327).** Add `on-file = "<command>"`
  to a `[[table]]` to derive that table's rows from each matched file's
  *contents*: `dirsql` runs the command once per file (the command reads the
  file and prints a JSON array of row objects on stdout), and each object
  becomes a row. Placeholders: `{path}` (the match relative to the index root,
  appended automatically when the template omits it), `{abspath}` (absolute
  path), and `{root}` (the index root). The command runs in the config file's
  directory with the inherited environment and a fixed 30-second timeout, no
  shell (argv-split with shell-like quoting; `sh -c '…'` is the explicit
  opt-in). JSON values map to SQLite as `null`→NULL, `bool`→`0/1`,
  integer→INTEGER / other number→REAL, string→TEXT, nested array/object→JSON
  TEXT. Filesystem facts (stat virtuals and glob captures) are still merged
  onto every row, with command-emitted columns winning. **Per-file error
  isolation:** a command that fails (non-zero exit, timeout, spawn error) or
  emits output that isn't a JSON array of objects skips only that file (with a
  stderr warning) and never aborts the scan. Handled entirely by the shared
  Rust core, so it is identical across the `pip` / `npm` / `cargo` installs.
  (#327)

- **Rust core: a reusable command runner (`dirsql::command::run_command`), the
  foundation for command-backed events (Epic B, #322 / B1 #326).** Splits a
  command template into argv with shell-like quoting but runs **no shell**
  (`sh -c '…'` is the explicit opt-in), substitutes named placeholders
  (`{path}`, `{args}`, `{abspath}`, `{root}`, …) into whole argv tokens so
  untrusted values stay a single injection-safe argument, and supports an
  append-if-absent placeholder. Runs the child in a given working directory
  with the inherited environment (so `uvx`/`npx` shims resolve), an optional
  stdin payload, and a timeout that kills a runaway child. The result payload
  is the last non-empty line of stdout (stderr is never data); a non-zero
  exit or timeout is a failure carrying the stderr tail. Exposed as
  the `command::{run_command, Placeholder, CommandOutput, CommandError}` items.
  Internal plumbing (a low-level core primitive, not re-exported at the crate
  root) with no user-facing (CLI/config) surface yet — the events that expose
  it in `.dirsql.toml`, and their user docs, land with `on-file` (#327),
  `pre-query` (#328), and `post-query` (#329). (#326)

- **TypeScript SDK: `new DirSQL({ extensions: [...] })` constructor option
  (restores parity with Rust #225 / Python #229).** Pass an array of
  `{ path, entrypoint? }` objects (`entrypoint` optional) to load SQLite
  extensions onto the connection at startup, marshaled through the napi
  binding into the shared Rust core (enable → load → disable). Programmatic
  entries load first, followed by any `[[dirsql.extension]]` entries from a
  `config` file (relative config paths resolve against the config's parent
  directory; programmatic paths are taken verbatim). Closes the last
  extension-loading parity gap in `PARITY.md`. (#230)

- **Rust SDK: load SQLite extensions via config (`[[dirsql.extension]]` /
  `DirSQLBuilder::extension`).** Declare a local extension shared-library path
  (with an optional `entrypoint` init-symbol override) and dirsql loads it onto
  the connection at startup, before any `CREATE TABLE`, then disables loading
  again so the SQL `load_extension()` function is never left exposed to later
  queries. Config-file paths resolve relative to the config's parent directory.
  Opt-in and additive: with no `[[dirsql.extension]]` entries, extension
  loading stays disabled. (#225)

- **Extension loading — review hardening (#225).** Load failures now surface as
  a dedicated `DirSqlError::Extension` naming the library (was a generic
  `DbError::Sqlite`); an empty `path = ""` is rejected at config-parse
  time; and a `CREATE VIRTUAL TABLE` `[[table]]` is rejected with a clear "not
  supported" error — extension-backed virtual tables are not dirsql-managed
  tables, so extensions provide functions for queries and regular-table DDL.

- **Python SDK: `DirSQL(extensions=[...])` constructor parameter (parity with
  Rust #225).** Pass a list of `{"path": ..., "entrypoint": ...}` dicts
  (`entrypoint` optional) to load SQLite extensions onto the connection at
  startup, marshaled into the shared Rust core (enable → load → disable).
  Programmatic entries load first, followed by any `[[dirsql.extension]]`
  entries from a `config` file (relative config paths resolve against the
  config's parent directory). TypeScript parity is tracked in #230. (#229)

- **Rust SDK: `DirSQLBuilder::poll_interval(Duration)`.** Tunes the channel-
  based `watch()` loop's poll cadence (default 200ms). Lower values trade
  idle CPU for tighter event-to-stream latency. Addresses P7 of #218.

- **Rust SDK: `impl From<String> for AppState`.** Symmetric with
  `From<DirSQL>` so the degraded `AppState::Unavailable` arm can be built
  with the same `.into()` ergonomics as the ready arm. Addresses I11 of
  #218.

- **Rust SDK: `db::validate_identifier` + `DbError::InvalidIdentifier`.**
  Validates table names (parsed from DDL) and column names (from
  `extract`-returned rows) against `[A-Za-z_][A-Za-z0-9_]*` before they are
  interpolated into formatted SQL. Closes the latent identifier-injection
  surface in `Db::create_table`, `Db::insert_row`, `Db::delete_rows_by_file`,
  and `Db::get_table_columns` — rusqlite's single-statement `execute`
  defangs most payloads today, but the validator surfaces the issue as a
  clean `InvalidIdentifier` error instead of relying on that accident.
  Addresses S1 of #218.

- **TypeScript SDK exports a `Table` class (parity with Python / Rust).**
  `import { Table } from "dirsql"` now resolves to a constructable class
  implementing `TableDef`. `new Table({ ddl, glob, extract, strict? })` is
  a thin identity wrapper — structurally identical to the equivalent plain
  object literal, with the same enumerable keys — so every call site that
  accepts `TableDef[]` (notably `new DirSQL({ tables: [...] })`) accepts
  either form interchangeably. Pre-existing TS code using plain object
  literals continues to compile and run unchanged. Parity-restoring with
  Python's `Table(ddl=..., glob=..., extract=...)` and Rust's
  `Table::new(...)` / `Table::strict(...)` / `Table::try_new(...)`. (#216)

- **Python SDK ships PEP 561 type information.** The wheel now bundles
  `dirsql/py.typed` and `dirsql/_dirsql.pyi`, so downstream consumers see
  types for `DirSQL`, `Table`, `RowEvent`, and `__version__` in editors and
  type checkers. The stub is hand-written and mirrors the PyO3 bindings in
  `packages/python/src/lib.rs`; any change to that source's public surface
  must update the stub in the same PR. Parity-restoring with the Rust core
  (types from source) and TypeScript SDK (types from generated `.d.ts`).

- **Strict type-checking in CI for the Python SDK (`ty`).** Adds
  `.github/workflows/python-typecheck.yml` running Astral's `ty` with every
  rule promoted to `"error"` (`--error all --error-on-warning`). Existing
  findings were initially frozen behind line-precise `# ty:ignore[<rule>]`
  baseline comments inserted by `ty check --add-ignore` -- a TODO list that
  has since been driven to zero (see _Fixed_), so the tree now type-checks
  clean with no suppressions. New errors in new or edited code fail
  CI immediately. Ruff `PGH003` is enabled to forbid bare `# type: ignore`
  / `# ty: ignore` without a rule code. Test files (`**/test_*.py`,
  `**/*_test.py`) are excluded for now because their `monkeypatch.setattr`
  fakes are a separately-tracked smell (AGENTS.md "Test Boundaries").

- **Dedicated TypeScript type-check job.** Adds
  `.github/workflows/ts-typecheck.yml` running `tsc --noEmit` standalone
  on every PR touching `packages/ts/**/*.ts`. `tsc` was already running
  inside the `ts-test.yml` build step (`tsconfig.json` has `strict`,
  `noUncheckedIndexedAccess`, `verbatimModuleSyntax`), but a dedicated
  job surfaces type errors in seconds without waiting on `napi build` --
  the native addon is loaded via `createRequire` at runtime, so type
  checking does not need `dirsql.node` or the generated `index.d.ts`.
  Parity with the new Python type-check job.

- **TypeScript SDK: top-level `parseTableName(ddl)` export** backed by the
  Rust `dirsql::db::parse_table_name`, and the PyO3 `Table` class exposes
  `extract` and `name` as readable attributes. (#196)

- **Zero-config `files` table.** Running the `dirsql` server in a
  directory with no `.dirsql.toml` now serves a default `files` table --
  one row per file under the directory, with the filesystem-fact columns
  `_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime` --
  instead of starting in the degraded (HTTP 503) state. `SELECT * FROM
  files` and `SELECT name FROM sqlite_master` work immediately in any
  directory; no ignores are applied, so every file is indexed. A
  `.dirsql.toml`, when present, fully overrules the default. (#184)

### Changed

- **Unit coverage for the Python and TypeScript SDKs is now enforced by
  testing-conventions `unit coverage`, measured unit-only.** The bespoke
  per-package floors (`pytest --cov --cov-fail-under` and the vitest
  `thresholds` block) are retired in favor of `testing-conventions unit
  coverage` (full + a PR-only `--base` changed-lines check), wired into
  `python-test.yml` / `ts-test.yml`. The floor is now measured over the
  colocated unit suite only -- integration tests no longer pad the metric --
  with per-package floors in `testing-conventions.toml`
  (`[python.coverage]` / `[typescript.coverage]`), held at 100%. Reaching 100%
  unit-only added the unit tests the combined run had let slip: the
  `_async.py` async-wrapper branches (Python) and `librarySlug`'s success path
  plus `loadNativeCore`'s default dirname resolver (TypeScript). The Rust core
  keeps its bespoke `cargo llvm-cov` job for now -- the tool can't yet measure
  it unit-only (#295). Separately, the testing-conventions CLI version is no
  longer pinned in CI (always installs the latest release). (#234)

- **TypeScript SDK: all `packages/ts/` filenames standardized to dash-case
  (kebab-case).** Source and test modules that were `camelCase` or
  `snake_case` were renamed (e.g. `loadNativeCore.ts` ->
  `load-native-core.ts`, `resolveBinary.ts` -> `resolve-binary.ts`,
  `from_config.test.ts` -> `from-config.test.ts`); single-word files
  (`index.ts`, `die.ts`, `main.ts`) are unchanged. Only filenames moved --
  exported symbols keep their `camelCase` / `PascalCase` names, and the
  package's public entry points (`dist/index.js`, `dist/cli/dirsql.js`) are
  unchanged, so installed consumers are unaffected. The convention is now
  enforced for `src/` and `test/` by biome's `style/useFilenamingConvention`
  rule and documented in `AGENTS.md`. (#193)

- **TypeScript SDK: lowered the supported Node floor to `>=20.11`** (from
  `>=22`). Nothing in the shipped SDK or CLI requires Node 22: the native
  addon targets the `napi6` ABI (Node 10/12/14+), the only runtime
  dependency (`smol-toml`) needs Node 18, and the newest API used
  (`import.meta.dirname`) landed in Node 20.11. Lowering the floor also lets
  a bare `npx dirsql` resolve to the current release on Node 20 instead of
  down-selecting to a bin-less pre-`0.1.11` version. (#246, #243)

- **Rust SDK: `DirSQL::query`'s `_dirsql_*` projection filter is comment-
  and string-literal-aware.** Previously the column-filter logic used
  `sql.contains("_dirsql_file_path")`, which leaked the tracking column
  when the name appeared only inside a `/* comment */` or
  `'string literal'`. The filter is now backed by `db::strip_sql_noise`,
  which removes comments and string literals before scanning for explicit
  `_dirsql_*` references. Quoted identifiers (`"_dirsql_file_path"`) still
  count as explicit. Addresses S2 of #218.

- **Rust SDK: `DirSqlError::Watch`, `Matcher`, `Config` carry an optional
  underlying source.** The three variants moved from `(String)` tuple form
  to `{ message: String, source: Option<Box<dyn Error + Send + Sync>> }`
  struct form. `Error::source()` now returns the underlying `notify`,
  `globset`, or `config::ConfigError` for downcasting / chained diagnostics.
  Display output is unchanged. Addresses I3 of #218; see `MIGRATIONS.md`
  for the pattern-matching update.

- **Rust SDK: `_ext` stat virtual preserves the file extension's original
  case.** Previously lowercased; now passes through verbatim so case-
  sensitive filesystems can distinguish `Photo.JPG` from `photo.jpg`.
  Consumers wanting case-insensitive matching can `LOWER(_ext)` in SQL.
  Addresses I8 of #218; see `MIGRATIONS.md`.

- **Rust SDK: `persist::PARSER_VERSIONS_JSON` is now `{}` (was the legacy
  parser-versions list).** Per-format parsing was removed in #169; the
  meta key was carrying dead metadata. Existing on-disk caches will be
  cleanly rebuilt on next startup via the normal `meta_is_compatible`
  rejection path. Addresses I6 of #218.

- **CLI launcher coverage exclude lifted; launchers unit-tested.** The
  Python `dirsql.cli.main` / `binary_path` / `is_windows` and the
  TypeScript `main` / `die` / `resolveBinary` helpers are now fully
  unit-tested. The coverage configs no longer omit the launcher
  directories wholesale -- only the `dirsql.ts` npm `bin` shim stays
  excluded. Tests fake out the launchers' module imports and process
  state via `unittest.mock.patch.object` (Python) and `vi.mock` /
  `vi.stubGlobal` (TypeScript); the launcher production signatures
  are unchanged. No user-facing behavior change. (#211)

- **CLI launcher directories renamed for cross-SDK consistency.** The
  Python `dirsql/_cli/` package and the TypeScript `src/bin/` directory
  are both renamed to `cli/`. The Python `[project.scripts]` entry-point
  moves from `dirsql._cli.main:main` to `dirsql.cli.main:main`; the npm
  `bin` field points at `dist/cli/dirsql.js` instead of
  `dist/bin/dirsql.js`. The user-facing `dirsql` command on PATH is
  unchanged. The Python leading underscore was misleading (the directory
  holds the public console-script entry, not internal-only code), and
  the TypeScript `bin/` vs. Python `_cli/` mismatch made cross-SDK
  references awkward. See `MIGRATIONS.md`. (#210)

- **`extract` callbacks no longer receive file content.** The `extract`
  callback on a programmatic `Table` (Rust/Python) / `TableDef`
  (TypeScript) now takes a single argument — the absolute filesystem
  path of the matched file — instead of `(path, content)`. `dirsql` no
  longer reads file bodies during the scan or watch loop; a callback
  that needs the file content reads it itself (`open(path)` /
  `std::fs::read_to_string(path)` / `readFileSync(path)`). This removes
  a vestigial eager UTF-8 read left over from the `format`/`each`
  config grammar deleted in #169, and lets a table glob match binary
  files without aborting the build. Breaking change across the Python,
  Rust, and TypeScript SDKs; `.dirsql.toml` config users are
  unaffected. See `MIGRATIONS.md`. (#184)

### Removed

- **Python 3.10 support dropped.** `requires-python` is now `>=3.11`
  (was `>=3.10`). `pip` / `uv` will refuse to install dirsql on Python
  3.10. This is required to ship 0.3.6: putitoutthere's multi-version
  wheel build (#369) fans a wheel row per `requires-python` version,
  and its `bundle_cli` wheel-content verify step `import tomllib` —
  stdlib only on CPython >= 3.11 — so the 3.10 row crashes the release
  build. Dropping 3.10 removes that row. 3.10 can be restored once the
  upstream verify step no longer depends on `tomllib`.

### Fixed

- **Python: `DirSQL(...)` no longer raises `TypeError` when neither `root`
  nor `config` is given.** The "no root" check is delegated to the core and
  surfaces from `await db.ready()` / `query()`, matching Rust. (#260)

- **Test files no longer ship in built distributions.** The Python wheel
  bundled the colocated `dirsql/**/*_test.py` unit tests, the npm tarball
  shipped compiled `dist/**/*.test.*`, and the crate shipped `tests/**`. Each
  is now excluded at build time (maturin `exclude`; a `tsconfig.build.json`
  that drops `*.test.ts` / `*.spec.ts` from the emitted `dist/`; and `tests/`
  in the crate's `[package].exclude`), so consumers no longer receive test
  code as package data. Enforced going forward by the new `packaging` gate
  (testing-conventions). (#238)

- **Python SDK: `await db.query(...)` before `await db.ready()` no longer
  raises `AttributeError`.** `query()` now awaits `ready()` itself, so a query
  issued before the background scan finishes waits for it (and re-raises any
  initialization error) instead of dereferencing the still-`None` internal
  handle. This matches the TypeScript SDK, where every method transparently
  awaits readiness (Rust's `AsyncDirSQL` stays explicit by design). The public
  `query(sql)` signature is unchanged.
- **Python SDK: the `ty` baseline is now empty — the source tree type-checks
  clean with no suppressions.** Resolved the four `# ty:ignore[...]` baseline
  comments seeded by the type-checker rollout: the unguarded `self._db.query`
  in `_async.py` (the bug above), the `cfg_dir` narrowing in
  `resolve_config.py`'s `_abs`, and the `missing-override-decorator` findings on
  `_async.py`'s `__dict__` property and the `_dirsql.pyi` stub's
  `RowEvent.__repr__` (both now carry `@override`, sourced from
  `typing_extensions` for the type checker only so Python 3.11 keeps working
  with no new runtime dependency).
- **Quoted and schema-qualified table names in `CREATE TABLE` DDL now resolve
  correctly.** `parse_table_name` — which provides the name dirsql indexes a
  table under, and backs the Python `Table.name` attribute and the TypeScript
  `parseTableName` export — previously used a hand-rolled splitter that stopped
  at the first space or `(` and kept any surrounding quotes. So a quoted
  identifier like `CREATE TABLE "comments" (...)` (the shape ORMs and schema
  generators routinely emit) resolved to `"comments"`, which then failed
  identifier validation and the table was rejected at registration. The name is
  now parsed by a small quote-aware tokenizer that strips the three SQLite
  quoting forms (`"..."`, `` `...` ``, `[...]`) and resolves a schema-qualified
  `main.comments` to the bare table segment. The function stays pure and
  synchronous; the fix lives in the shared Rust core, so all three SDKs (Rust,
  Python, TypeScript) are fixed with no binding changes. (#204)

- **File watcher now emits events when `root` is relative.** Building a
  `DirSQL` with a relative root (e.g. `DirSQL::new("./data", tables)` or
  `DirSQL::new(".", tables)`) handed that relative path straight to `notify`,
  which misbehaves on relative paths — depending on platform it delivered
  **no events at all** or delivered them under the cwd-joined absolute path
  so the `_path` virtual column leaked the absolute prefix. The initial scan
  was unaffected; only the live `watch()` / `poll_events()` stream was broken.
  The watcher now runs against a canonicalized watch-root (matching the CLI,
  which already did this), while the user-supplied `root` is preserved
  verbatim — so the initial scan, `config()` serialization, and `_path`
  output are unchanged. Fix lives in the shared Rust core, so all three SDKs
  (Rust, Python, TypeScript) are fixed with no binding changes. (#250)

- **`npx dirsql` works on Linux hosts with glibc < 2.39 again.** The
  npm `bundled-cli` Linux binary in 0.3.11 / 0.3.12 was dynamically
  linked against `GLIBC_2.39` and crashed at startup on Ubuntu 22.04,
  Debian 12, Amazon Linux 2, and any other host whose runtime glibc
  predates Ubuntu 24.04's. Root cause was a missing
  `[package.bundle_cli]` subtable on the `dirsql-npm` package in
  `putitoutthere.toml`: every musl / cross-compile / stage / verify
  step in upstream `_matrix.yml`'s npm bundled-cli row is gated on
  `matrix.bundle_cli`, so without the subtable nothing in the upstream
  pipeline produced a binary, and the consumer's local
  `npm run build` (`packages/ts/tools/stagePlatform.ts`) silently fell
  through to plain `cargo build --target x86_64-unknown-linux-gnu` on
  the ubuntu-latest runner. Adding the subtable (mirroring the pypi
  block) makes upstream cross-compile against the musl triple and stage
  the static binary into `packages/ts/build/bundled-cli-{triple}/` after
  `npm run build` (putitoutthere#386 ordering fix), so the static binary
  is what reaches the upload-artifact step. A new vitest in
  `packages/ts/tools/putitoutthereConfig.test.ts` keeps the invariant
  honest: any future package that declares
  `build = [..., { mode = "bundled-cli", ... }, ...]` must also declare
  the matching `[package.bundle_cli]` subtable. (#189)

- **`npx dirsql` and `uvx dirsql` work end-to-end again.** 0.3.5 published
  but the CLIs were still broken: the npm-bundled binary was packed
  without the executable bit (`spawnSync ... EACCES`) and stamped with a
  stale version, and PyPI shipped a cp312-only wheel so non-3.12
  interpreters fell back to a sdist build with no bundled binary. All
  three are fixed upstream in putitoutthere (`bundled-cli` now `chmod
  +x`es the staged binary and rewrites its version; `pypi` builds one
  wheel per `requires-python` version). dirsql consumes the fixes via
  `release.yml`'s `@v0` pin; 0.3.6 is the first release built by the
  corrected pipeline.
- **Release pipeline no longer fails publishing the crate to crates.io.**
  Removed the `packages/rust/target` symlink (and its root `.gitignore`
  exception) that worked around putitoutthere's `bundle_cli`
  workspace target-dir lookup. `cargo publish` followed the tracked
  symlink and archived the entire workspace build tree, producing a
  ~133 MiB `.crate` that crates.io rejected with `413 Payload Too
  Large` (10 MiB cap) -- failing the `release / publish` job and
  blocking every release after 0.3.4. putitoutthere #337 fixes the
  underlying workspace lookup, so the symlink (and its workarounds) are
  no longer needed.
- **Python wheel ships the `dirsql` CLI again.** Restored
  `[project.scripts] dirsql = "dirsql._cli.main:main"` in
  `packages/python/pyproject.toml` and declared `[package.bundle_cli]`
  in `putitoutthere.toml` (requires putitoutthere ≥ v0.2.17). The
  reusable workflow now cross-compiles the `dirsql` bin per target with
  `--features cli`, stages it into `dirsql/_binary/`, and
  maturin's `[tool.maturin].include` glob bundles it into each wheel.
  `pip install dirsql && dirsql ...` and `uvx dirsql` work end-to-end;
  upstream verifies wheel contents post-build.

### Added

- **PR-time config-sanity gate.** New
  `.github/workflows/release-config-check.yml` calls putitoutthere's
  `check.yml@v0` reusable workflow on every pull request. Validates
  `putitoutthere.toml` (parse + schema + common-mistakes detector,
  unique package names, `depends_on` cycle / dangling-ref detection,
  glob coverage, tag-format collisions), npm `repository` field, crates
  `description` / `license`, pypi `bundle_cli` binary declaration, and
  npm target triple mapping. Few seconds per PR, no per-target build.
  Companion to the existing `release-precheck.yml` build-matrix gate.

- **`pack-install` build-CI job for the npm package.** Builds the real
  `dirsql` binary, packs the host's `@dirsql/cli-<slug>` sub-package and
  the main `dirsql` package, installs both into a fresh dir, and runs
  `node_modules/.bin/dirsql --version`. Companion to the Python
  wheel-install job; gates publishability of the npm artifact.
- **AGENTS.md test-boundary rules.** Integration tests target the
  public SDK API (not the CLI launcher); e2e tests have no mocks /
  fakes / monkeypatching; monkeypatching production-module attributes
  is a code smell and is not allowed (use dependency injection via the
  public API or fixtures).

### Removed

- **Monkeypatch unit tests for the CLI launcher.** Deleted
  `packages/python/python/dirsql/_cli/{main,binary_path,is_windows}_test.py`,
  `packages/ts/ts/bin/{main,resolveBinary,die}.test.ts`, and
  `packages/ts/test/{cli.test.ts,runLauncher.ts,fakeInstallRoot.ts}`. The
  launchers are thin enough that the only meaningful coverage is the
  build-CI smoke tests above; the deleted tests monkeypatched production
  module attributes (`os.execv`, `process.exit`, `process.stderr`,
  `binary_path`, `resolveBinary`) which the new test-boundary rules
  forbid.
- **Content parsing is no longer a dirsql concern.** `parser.rs` and the
  `Format` enum (`Json` / `Jsonl` / `Csv` / `Tsv` / `Toml` / `Yaml` /
  `Frontmatter`) are gone, along with `ColumnSource`, `apply_columns`,
  `parse_file`, `infer_format`, and every `parse_*` / `navigate_*` helper.
  The corresponding `format`, `each`, and `[table.columns]` keys in
  `.dirsql.toml` are no longer recognized as part of the grammar (they parse
  but are ignored). The `csv` and `serde_yaml` Cargo dependencies are
  dropped. Closes #169.
- `DirSqlError::NoFormat` and `ConfigError::UnknownFormat` variants.

### Changed

- **`[[table]]` entries in `.dirsql.toml` now produce filesystem-fact rows
  instead of parsed-content rows.** Each matched file emits one row; columns
  come from glob path captures (named `{placeholder}` segments) and reserved
  stat virtuals (`_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`,
  `_ctime`). The DDL declares which subset of these the SQL table exposes;
  undeclared keys are silently dropped during normalization (in non-strict
  mode) or rejected (in strict mode).
- **All tables — programmatic and config-driven — now auto-inject glob
  captures and stat virtuals into every row** (filtered to the DDL's
  declared columns). User-extract values win over auto-injected values when
  keys collide. Programmatic `Table::new(...)` users no longer need to parse
  the path themselves to surface capture columns or stat metadata.
- `ARCHITECTURE.md` now opens with an explicit scope statement — dirsql is
  a queryable index over a filesystem; content interpretation is out of
  scope.

### Added

- **`dirsql init` subcommand.** Generates a starter `.dirsql.toml` for a
  directory by shelling out to the local `claude` CLI. Filesystem-fact
  only: emits `[[table]]` blocks whose columns come from glob path
  captures and reserved stat virtuals (`_path`, `_basename`, `_dir`,
  `_ext`, `_size`, `_mtime`, `_ctime`). Bails before invoking `claude`
  if the output already exists; `--force` overrides. Refuses to run
  with a clear error if `claude` is not on `PATH`. Flags: `--root`,
  `--output`, `--force`. See `docs/guide/init.md`. Closes #96.

- **Persistent on-disk SQLite cache.** Opt-in `persist` / `persist_path`
  option on `DirSQL` (and `[dirsql] persist = true` in
  `.dirsql.toml`). When enabled, the database is written to
  `<root>/.dirsql/cache.db` (override via `persist_path`) so
  subsequent startups only re-parse files that have actually changed.
  Reconcile uses size + mtime when the mtime is safely outside the
  racy window, falling back to a BLAKE3 content hash otherwise.
  Glob/DDL changes and a `dirsql_version` bump force a full rebuild,
  so SQL state never silently disagrees with the filesystem after
  reconcile. `.dirsql/` is unconditionally excluded from the scan
  walk. Available across all three SDKs (Rust `DirSQL::builder().persist(..)`,
  Python `DirSQL(..., persist=True)`, TypeScript `new DirSQL({ ..., persist: true })`).
  Closes #95.

### Fixed

- **PyPI wheels now ship at the planned release version.** Previously
  every wheel shipped with `0.1.0` baked in regardless of what the
  release plan computed, because `packages/python/pyproject.toml`
  declared a static `version = "0.1.0"` literal and maturin reads that
  field verbatim with no env override. Switched to maturin's
  dynamic-version mode (`dynamic = ["version"]` in `pyproject.toml`,
  with the literal moved to `packages/python/Cargo.toml`'s
  `[package].version`). Putitoutthere's `write-version` step
  (added upstream in thekevinscott/putitoutthere#277) rewrites the
  Cargo.toml literal before `maturin build`, so wheels ship at the
  planned version. Closes #166.

### Changed

- **Release pipeline rewritten on top of [putitoutthere](https://github.com/thekevinscott/putitoutthere).**
  The hand-rolled `patch-release.yml` + `publish.yml` + `publish-npm.yml`
  + `release-scripts.yml` + cargo-dist `release.yml` stack is replaced
  with a single `.github/workflows/release.yml` that calls the reusable
  `thekevinscott/putitoutthere/.github/workflows/release.yml@v0` workflow.
  Configuration moves to `putitoutthere.toml` at the repo root. Auth is
  OIDC trusted publishers on all three registries — no long-lived
  registry tokens. Per-package tags replace the single shared `v{version}`
  tag (each of `dirsql-rust`, `dirsql-py`, `dirsql-npm` now tags as
  `<name>-v<version>`); historical `v0.2.x` tags remain untouched. See
  [MIGRATIONS.md](./MIGRATIONS.md) for the consumer-visible details.
- **npm build tooling collapsed to a single per-host staging script.**
  `packages/ts/tools/stagePlatform.ts` runs `napi build --release` and
  `cargo build --release --bin dirsql --features cli --target <triple>`
  for the host triple, then stages outputs at
  `packages/ts/build/napi-{triple}/` and
  `packages/ts/build/bundled-cli-{triple}/` for putitoutthere's
  npm-platform handler to package. Replaces the cargo-dist-driven
  `buildPlatforms.ts` / `buildLibPlatforms.ts` / `buildOne.ts` /
  `buildLibOne.ts` / `extract.ts` / `findBinary.ts` /
  `syncVersion.ts` cross-compile pipeline; each putitoutthere matrix
  row now runs on a runner native to its target, so cross-compilation
  is no longer needed.

### Added

- **`.github/workflows/bootstrap-npm-platforms.yml`.** One-time
  manually-dispatched workflow that publishes `0.0.0-bootstrap` stubs
  for the ten per-platform sub-packages (`@dirsql/lib-*` and
  `@dirsql/cli-*`) using a long-lived `NPM_TOKEN`. Required because
  npm's trusted-publisher feature can't be enabled on a package that
  doesn't yet exist, and putitoutthere v0.2 deliberately doesn't pass
  long-lived tokens through the reusable workflow. Delete the workflow
  + secret after bootstrap completes and the trusted publishers are
  registered.

### Removed

- **PyPI wheels no longer ship the `dirsql` CLI binary.** Putitoutthere
  v0.2.3's `[package.bundle_cli]` recipe is parsed but its reusable
  workflow has no step that cross-compiles + stages the binary, so
  declaring the block would silently produce wheels missing the binary.
  Block is dropped from `putitoutthere.toml` (and the matching
  `[project.scripts] dirsql` entry from `pyproject.toml`) until the
  upstream gap closes. CLI install paths during this window:
  `cargo install dirsql --features cli` or `npx dirsql`.
- `scripts/release/` (custom Python orchestration: `compute_version.py`,
  `check_published.py`, `resolve_publish_targets.py`, plus their tests
  and `pyproject.toml`). Functionally replaced by putitoutthere's
  built-in cascade detection, version computation, and `isPublished`
  pre-check.
- `dist-workspace.toml` and the orphaned `[profile.dist]` block in the
  workspace `Cargo.toml`. cargo-dist no longer cuts the release.
- `.github/workflows/patch-release.yml`, `.github/workflows/publish.yml`,
  `.github/workflows/publish-npm.yml`, `.github/workflows/release-scripts.yml`.

### Added

- `dirsql` CLI binary (Rust, `--features cli`). Running the binary
  starts a long-lived HTTP server bound to `localhost:7117` that
  exposes the SDK over the network:
  - `POST /query` — JSON-in, JSON-rows-out.
  - `GET /events` — Server-Sent Events stream of row change events;
    payloads mirror `DirSQL::watch()`'s `RowEvent`.
  - Graceful shutdown on `SIGINT` / `SIGTERM` that drains in-flight
    requests and closes any attached SSE streams.
- Opt-in `cli` Cargo feature. Library consumers (`cargo add dirsql`)
  pull zero CLI dependencies; `cargo install dirsql --features cli`
  builds the binary.
- Distribution scaffolding:
  - `cargo-dist` config (`dist-workspace.toml` + auto-generated
    `.github/workflows/release.yml`) producing per-target archives on
    every `v*.*.*` tag.
  - Per-platform npm sub-packages published under `@dirsql/cli-*`,
    driven by `packages/ts/tools/buildPlatforms.ts` from the cargo-dist
    archives. Main `dirsql` npm package gains a `bin/dirsql.js`
    launcher and an `optionalDependencies` list that picks the right
    sub-package at install time (esbuild/biome/swc pattern).
  - PyPI wheels bundle the Rust binary directly: the
    `.github/workflows/publish.yml` build job stages
    `packages/python/python/dirsql/_binary/` before `maturin build`,
    and `[tool.maturin] include` ships it as package data. The new
    `dirsql._cli.main:main` console-script execs it. Pip wheel tags
    handle the platform dispatch.
- Documentation:
  - `docs/guide/cli.md` — HTTP server, flags, endpoints, SSE schema,
    "why SSE" rationale.
  - `packages/rust/README.md` now distinguishes the library install
    from the opt-in CLI install and calls out the `required-features`
    silent-skip footgun.
  - `CHANGELOG.md` (this file).
  - Three-tier docs structure: root `README.md` mirrors the layout of
    `docs/` (one `##` per page) so agents reading the source can
    navigate without leaving the repo, every page in `docs/` has a
    `canonical` frontmatter + visible link back to its published URL,
    and the `docs/` folder now ships inside each published SDK
    package (`packages/<lang>/docs` symlinks the workspace `docs/`;
    cargo, maturin, and the npm `prepack` staging script include the
    markdown content in published artifacts).
- Tests:
  - 11 in-process HTTP integration tests
    (`packages/rust/tests/cli_integration.rs`) covering every
    documented endpoint, error class, method mismatch, and graceful
    shutdown.
  - 9 e2e tests (`packages/rust/tests/cli_e2e.rs`) that spawn the
    compiled binary and drive it over real HTTP / SSE / filesystem
    mutations.
  - Full TypeScript unit-test coverage (16 cases) for the npm launcher
    + build tooling, vitest-reported 100% lines / 95%+ branches /
    100% functions.
  - Python launcher tests migrated to `pytest_describe` blocks.

### Notes for maintainers

- Required repo secrets for the first tagged release:
  - `NPM_TOKEN` (publishes `dirsql` and `@dirsql/cli-*`).
  - `PYPI_API_TOKEN` (already used by `publish.yml`; trusted publisher
    works too).
- `@dirsql` npm scope must exist and be owned by the release account.
