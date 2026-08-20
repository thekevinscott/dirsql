# Architecture

## Scope

`dirsql` is a queryable index over a local filesystem. Files are rows; the
database is the index. Where a table's columns come from depends on the table
kind, and dirsql never injects a column the table did not produce:

- **Named tables** (`[[table]]` / SDK `Table`) have exactly the columns their
  `on_file` hook emits, narrowed to the DDL. A hook is required; a hook-less
  `[[table]]` is a config-load error (every row would be all-NULL). A hook that
  wants the path or stat metadata computes it from the path it receives.
- **Path-tables** (`FROM './'`) expose seven filesystem stat columns (`path`,
  `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`) plus a lazily-read hidden
  `content` column — the one place dirsql supplies columns for you. Attaching a
  `--on-file` parser replaces the stat columns with the parser's output.

There is no automatic fact injection and no glob capture: a `{name}` glob
segment is rewritten to `*` and captures nothing. (Both mechanisms were
removed in the fact-removal epic, [#624](https://github.com/thekevinscott/dirsql/issues/624).)

**Content interpretation is intentionally out of scope.** dirsql does not
parse markdown frontmatter, JSON, CSV, YAML, TOML, or any other file format
on the user's behalf. If a project needs columns derived from file content,
the consumer registers a programmatic `Table` whose `on_file` callback does
the parsing in the host language (Python / TypeScript / Rust).

This scope is a deliberate inversion of the original design and was settled
in [issue #169](https://github.com/thekevinscott/dirsql/issues/169). The
prior `[table.columns]` source-dispatch and the `format` / `each` config
keys were ripped out; the per-format parser zoo (`Format::Json`, `Csv`,
`Yaml`, `Toml`, `Frontmatter`, …) is gone.

## Read-only by design

**dirsql never modifies the directory it indexes.** It opens files for reading
and does nothing else -- no writes, no truncation, no in-place rewrites, no
moves, no deletes, no permission or timestamp changes. A user can point dirsql
at any directory, including one with no backup, and the worst outcome is that
files are read.

This is a permanent property of the design, not an unimplemented feature.
Write-back -- letting a SQL `UPDATE` flow back into the files it came from --
is rejected as a **feature class**, not deferred. The filesystem is the source
of truth and the database is a derived view of it; a derived view that can
mutate its own source is no longer derived, and every consistency question it
raises (partial writes, conflicting concurrent edits, reconciling a failed
write against a watcher event) is a question dirsql exists to avoid. Anything
that mutates files belongs in the user's own code, where its failure modes are
the user's to reason about.

The guarantee is enforced at the query layer too: `query()` accepts only
statements SQLite's `sqlite3_stmt_readonly` classifies as reads, so a `DELETE`
against the in-memory index is refused rather than silently discarded.

### Exact scope

Three boundaries keep the guarantee honest -- it claims neither more nor less
than it delivers:

1. **Hooks run user commands, and those may write.** An `on-file` hook is an
   arbitrary command the user configured; dirsql executes it and reads its
   stdout. If that command writes files, files get written. The guarantee is
   that **dirsql itself** never writes -- not that a dirsql invocation is
   incapable of causing a write.
2. **Persist mode writes one cache database, and it is opt-in.** With
   `--persist` (off by default), dirsql maintains a SQLite cache so a restart
   need not re-parse unchanged files. By default this lands at
   **`<root>/.dirsql/cache.db` -- inside the indexed directory** -- creating
   the `.dirsql/` directory if absent; `--persist-path` puts it anywhere you
   name. This is the one path dirsql writes to, it is never a file dirsql
   indexed, and without `--persist` nothing is written at all.
3. **The index itself is ephemeral and off to the side.** Without persist, the
   database is an anonymous disk-backed temp database discarded on shutdown.

## Core Principle: One Implementation, Thin Bindings

**The Rust crate (`packages/rust/`) is the single source of truth for all business logic.** Every language SDK is a thin binding layer that wraps it -- it does NOT reimplement it.

- **`packages/rust/`** -- the `dirsql` Rust crate. All business logic lives here: SQLite operations, glob matching, file scanning, row diffing, file watching, plus the ergonomic user-facing Rust API (builder pattern, async support via tokio) and, behind the `cli` feature, the CLI itself (`src/cli/`). This is the only crate published to crates.io.
- **`packages/python/`** -- PyO3 bindings wrapping `dirsql`. Thin glue code + async Python wrapper. The underlying binding crate (`dirsql-py-ext`) is not published to crates.io.
- **`packages/ts/`** -- the `dirsql` npm package. The TypeScript SDK sources live under `src/`; the napi-rs binding crate (`dirsql-napi`) is colocated under `napi/`, built into the `.node` addon the SDK loads at runtime. The binding crate is a Cargo workspace member but is not published to crates.io.

**Never reimplement core logic in a language SDK.** If you're writing SQLite operations, glob matching, file scanning, or row diffing in Python or TypeScript, that code belongs in the Rust crate with a binding exposed to the SDK. The entire point of this architecture is a fast Rust core with language bindings, not three independent implementations.

### The CLI is the binding path, not a separate binary (#721)

The shipped `dirsql` CLI **is** the in-process binding path. `packages/rust/src/cli/` exposes `run_cli(argv) -> i32`; each binding re-exports it (pyo3 `run_cli`, napi `runCli`) and each launcher calls it in its own process. `cargo install dirsql --features cli` still produces a standalone executable — a ~20-line shim over the same `run_cli`, so every entry path runs identical code.

This reverses an earlier statement that the CLI was "a pure Rust binary that never crosses a binding". It did, and that cost every package a second copy of the core: the wheel shipped an extension module *plus* a bundled binary, and npm published `@dirsql/cli-*` alongside `@dirsql/lib-*`.

Both registries are now measured on published artifacts, not projected. For npm, **−42.8%** of the per-platform native payload (10,139,000 → 5,799,904 B), one artifact per platform instead of two. For pypi, the compressed wheels published to the index (`0.4.14`, the last release carrying the bundled binary, against `0.4.15`, the first without it):

| wheel | 0.4.14 | 0.4.15 | delta |
| --- | ---: | ---: | ---: |
| `macosx_10_12_x86_64` | 4,680,619 | 2,665,292 | −43.06% |
| `macosx_11_0_arm64` | 4,390,969 | 2,502,594 | −43.01% |
| `manylinux_2_39_aarch64` | 4,735,237 | 2,686,944 | −43.26% |
| `manylinux_2_39_x86_64` | 5,009,679 | 2,834,832 | −43.41% |
| `win_amd64` | 4,835,120 | 2,792,775 | −42.24% |
| **all five** | **23,651,624** | **13,482,437** | **−43.00%** |

The sdist grows 387,241 → 393,115 B (+1.52%), which is expected: it ships source, and the relocation added some.

Two numbers from the planning phase should not be quoted as the wheel result. #717 measured the bundled binary at **5,574,232 B** — that is the *uncompressed* file removed, not the compressed delta. The spike's **−50.8%** was the *installed native payload* (9,992,536 → 4,916,576 B), again uncompressed. Compressed wheels shrink less because the binary and the extension module share compressible content; **−43.00%** is the figure a user's download actually changes by.

Two things fell out of the change and are worth keeping in mind before touching this area:

- **Process semantics are per-launcher, under one shared contract.** Both launchers front the same `run_cli`, but each host needs different wiring to reach the same observable behavior. Node must install a signal listener *before* the call or a signalled process wedges (signal-hook chains to the prior handler; bare Node leaves `SIG_DFL`, which it does not emulate). CPython must *stop* `default_int_handler`'s `KeyboardInterrupt` from overwriting the core's graceful exit code. Both were measured, not reasoned about; the details live in each package's migration fragment.
- **`run_cli` always returns.** It never terminates the host process, and its codes are ordinary status codes — never 130/143. An embedder stays in control of its own exit.

## Cross-Language Parity

Aim for **complete API parity across all three SDKs**: same concepts, same capabilities, same naming where possible. Exceptions are allowed for language-idiomatic patterns:

- **Python**: `await db.ready()` (method call). snake_case. Async iterators for event streams.
- **TypeScript**: `await db.ready` (awaitable property). camelCase. AsyncIterables for event streams.
- **Rust**: Builder pattern or `db.ready().await`. snake_case. Stream trait for event streams.

When adding a feature to one SDK, file GitHub issues for the other two.

## Overview

`dirsql` is a Rust core with language-specific SDK wrappers.

```
┌─────────────────────────────────┐
│         Python SDK              │
│   DirSQL, Table, RowEvent       │
├─────────────────────────────────┤
│         PyO3 bindings           │
│   packages/python/src/lib.rs    │
├─────────────────────────────────┤
│         Rust crate              │
│   packages/rust/src/            │
│   ┌───────┬──────────┬────────┐ │
│   │  db   │ scanner  │watcher │ │
│   │       │          │        │ │
│   │SQLite │ glob     │notify  │ │
│   │in-mem │ matching │inotify │ │
│   └───────┴──────────┴────────┘ │
│   ┌───────┬──────────┐          │
│   │differ │ matcher  │          │
│   │row    │ glob →   │          │
│   │diffing│ table    │          │
│   └───────┴──────────┘          │
└─────────────────────────────────┘
```

## Rust crate (`packages/rust/`)

The `dirsql` Rust crate handles all heavy lifting:

### `db` -- Ephemeral SQLite

Wraps `rusqlite` with an anonymous disk-backed temp database (#402) — ephemeral like `:memory:`, but index pages spill to disk so resident memory does not scale with the corpus. Handles DDL execution (run verbatim -- no injected columns, epic #358), row insertion with per-file ownership recorded in the internal `_dirsql_internal_rows` table, querying, and row deletion by file path. The internal bookkeeping tables (`_dirsql_internal_rows`, `_dirsql_files`, `_dirsql_meta`) are a private surface: a SQLite authorizer installed on the `query()` path denies any read (or schema `PRAGMA`) targeting the reserved `_dirsql_*` namespace, so they are unreachable through the public query surface (issue #378) while the engine still writes them in the same transaction as the user rows.

**A table's name is declared, never derived.** Both `[[table]]` and the SDK
`Table` take a required `name`; nothing tokenizes the `CREATE TABLE` head of
`ddl` to work it out. After the DDL runs, SQLite's own catalog
(`pragma_table_list`) settles whether a table by that name exists — a load-time
error when it does not. That keeps quoted, schema-qualified and
`IF NOT EXISTS` DDL working without dirsql owning a SQL tokenizer, per *never
reinvent what SQLite expresses natively*. `validate_identifier` remains: `name`
is spliced into `format!()`-built INSERT/DELETE SQL, so it is still the
injection guard.

**Named tables are real; path-tables are virtual.** A declared `[[table]]` (or programmatic `Table`) is a real SQLite table whose rows are inserted on build and maintained by the watcher — the `db` module above. A [path-table](../docs/reference/path-tables.md) (`SELECT * FROM './'`, epic path-as-table) is a `dirsql_path` **virtual table** (`vtab.rs` / `path_table.rs`, rusqlite's `vtab` feature): no rows are stored, no reconcile or watcher runs, and the filesystem is walked live at query time (`xFilter`/`xNext` enumerate matched files; `xColumn` supplies stat values and lazily reads `content`). SQLite stays the entire query engine either way; the vtab only enumerates rows and supplies column values. Path-tables are registered on demand — see *Query execution* below.

### `scanner` -- Directory traversal

Walks a directory tree and matches files against table globs. Returns a list of `(file_path, table_name)` pairs. Uses the `matcher` module internally.

### `matcher` -- Glob-to-table mapping

Maps glob patterns to table names and handles ignore patterns. A file is matched against every glob in registration order; every matching pattern fires, so a file can belong to multiple tables. A `{name}` placeholder in a glob is rewritten to `*` before compilation, so it matches a single path segment but captures no value (glob captures were removed in #624).

### `watcher` -- Filesystem monitoring

Wraps the `notify` crate to watch for filesystem changes. Emits `FileEvent` variants: `Created`, `Modified`, `Deleted`. Uses a channel-based architecture where events are sent from a background thread and received via `recv_timeout` and `try_recv_all`.

### `differ` -- Row diffing

Compares old and new row sets for a file to produce `RowEvent` variants: `Insert`, `Update`, `Delete`, `Error`. Rows are compared by position (index within the file).

### Named-table rows (in `lib.rs`)

A named table's rows come entirely from its `on_file` hook — the core injects
no columns. The `on_file` callback receives only the matched file's absolute
path; dirsql does not read file contents on its behalf. A callback that needs
the file body, path parts, or stat metadata computes them from that path and
emits them as ordinary keys.

The returned keys are filtered to the columns declared in the table's DDL (via
`db.get_table_columns`) before SQLite insertion; keys not in the DDL are
dropped. This is the sole transformation the core applies to hook output.

Because a hook is the only column source, a `[[table]]` with no `on-file` is
rejected at config load (`ConfigError::HooklessTable`) — every row would be
all-NULL. The stat-fact injection layer and glob captures that formerly filled
such tables were removed in the fact-removal epic
([#624](https://github.com/thekevinscott/dirsql/issues/624)); stat columns now
live only on path-tables (`dirsql_path` virtual table, see Query execution).

## Python SDK (`packages/python/`)

### PyO3 bindings

The `lib.rs` file in `packages/python/src/` defines the PyO3 bindings that expose the Rust core to Python:

- `Table` (PyO3 class) -- stores DDL, glob, and the Python on_file callable
- `DirSQL` (PyO3 class) -- owns the database, table configs, file-row tracking, and watcher
- `RowEvent` (PyO3 class) -- represents a row-level change event

The Python `on_file` callable is called from Rust via PyO3's GIL-acquiring mechanism. Python dicts are converted to `HashMap<String, Value>` for storage, and converted back for query results.

### DirSQL (Python-facing async wrapper)

The public `DirSQL` class (`_async.py`) is a pure-Python async wrapper that uses `asyncio.to_thread` to run the synchronous Rust operations off the event loop. The constructor is sync (starts a background scan), `ready()` and `query()` are async, and `watch()` returns an async iterator that polls for events in a background thread. The Rust-backed `PyDirSQL` class is imported as `_RustDirSQL` internally and is not part of the public API.

## Data flow

### Startup scan

1. Python creates `DirSQL` with root path and table definitions
2. Rust executes DDL to create SQLite tables
3. `scanner` walks the directory and matches files to tables
4. For each matched file, Python `on_file` is called via PyO3
5. The returned rows are filtered to the DDL's declared columns (no columns are
   injected); rows are the hook's output alone
6. Rows are inserted into SQLite with tracking metadata
7. File-to-rows mapping is stored for later diffing

### File change processing

1. `notify` detects a filesystem event (create/modify/delete)
2. The matcher checks if the file belongs to a table
3. For create/modify: `on_file` is called with the file's absolute path
   (reading the file itself if it needs the body); its rows are filtered to the
   DDL's columns (no injection), then `differ` compares old and new rows
4. For delete: old rows are retrieved, all emitted as delete events
5. SQLite is updated (old rows deleted, new rows inserted)
6. `RowEvent` objects are returned to Python

### Query execution

1. Python calls `db.query(sql)`
2. A SQLite authorizer is installed for the prepare, denying reads of the internal `_dirsql_*` bookkeeping tables (issue #378); a query touching one fails with a "not authorized" error
3. Rust prepares the SQL against the ephemeral SQLite database. On a `no such table: X` error where `X` is path-shaped (`./`, `../`, `/`, `~/`), it registers a `dirsql_path` virtual table for `X` in the `temp` schema and re-prepares; the loop repeats until prepare succeeds or the error is not path-shaped. A bare glob (`'**/*.md'`) is left unresolved but its error gains a `did you mean './**/*.md'?` hint; an ordinary typo is left untouched. Named tables always win — the fallback fires only for names SQLite could not resolve (epic path-as-table)
4. Rust executes the prepared statement
5. Results are converted from `HashMap<String, Value>` to Python dicts -- exactly the user's declared columns, no filtering (epic #358)
