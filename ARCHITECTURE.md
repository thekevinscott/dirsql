# Architecture

## Scope

`dirsql` is a queryable index over a local filesystem. Files are rows; the
database is the index. Columns of any dirsql table come from filesystem-level
facts:

- The path itself, and parts derived from it (`path`, `basename`, `dir`,
  `ext`).
- Named captures in the glob pattern (`posts/{thread_id}/*.md` →
  `thread_id`).
- Stat metadata (`size`, `mtime`, `ctime`).

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

- **`packages/rust/`** -- the `dirsql` Rust crate. All business logic lives here: SQLite operations, glob matching, file scanning, row diffing, file watching, plus the ergonomic user-facing Rust API (builder pattern, async support via tokio). This is the only crate published to crates.io.
- **`packages/python/`** -- PyO3 bindings wrapping `dirsql`. Thin glue code + async Python wrapper. The underlying binding crate (`dirsql-py-ext`) is not published to crates.io.
- **`packages/ts/`** -- the `dirsql` npm package. The TypeScript SDK sources live under `src/`; the napi-rs binding crate (`dirsql-napi`) is colocated under `napi/`, built into the `.node` addon the SDK loads at runtime. The binding crate is a Cargo workspace member but is not published to crates.io.

**Never reimplement core logic in a language SDK.** If you're writing SQLite operations, glob matching, file scanning, or row diffing in Python or TypeScript, that code belongs in the Rust crate with a binding exposed to the SDK. The entire point of this architecture is a fast Rust core with language bindings, not three independent implementations.

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

### `scanner` -- Directory traversal

Walks a directory tree and matches files against table globs. Returns a list of `(file_path, table_name)` pairs. Uses the `matcher` module internally.

### `matcher` -- Glob-to-table mapping

Maps glob patterns to table names and handles ignore patterns. A file is matched against every glob in registration order; every matching pattern fires, so a file can belong to multiple tables. Glob patterns may contain `{name}` capture placeholders; the matcher returns captured segments alongside each table name.

### `watcher` -- Filesystem monitoring

Wraps the `notify` crate to watch for filesystem changes. Emits `FileEvent` variants: `Created`, `Modified`, `Deleted`. Uses a channel-based architecture where events are sent from a background thread and received via `recv_timeout` and `try_recv_all`.

### `differ` -- Row diffing

Compares old and new row sets for a file to produce `RowEvent` variants: `Insert`, `Update`, `Delete`, `Error`. Rows are compared by position (index within the file).

### Filesystem-fact injection (in `lib.rs`)

The `on_file` callback receives only the matched file's absolute path; dirsql
does not read file contents on its behalf. A callback that needs the file body
reads it itself. After `on_file` returns, and before SQLite insertion, the core
merges two sources of filesystem-derived columns into every row:

- **Glob path captures**, by capture name (`{thread_id}` → `thread_id`).
- **Stat columns**, under bare names: `path`, `basename`, `dir`, `ext`,
  `size`, `mtime`, `ctime`. These are ordinary stored columns, not SQLite
  `GENERATED ... VIRTUAL` columns -- "stat" describes where the value comes
  from, not how it is stored. See `docs/reference/columns.md`.

Auto-injected keys are filtered to the columns declared in the table's DDL
(via `db.get_table_columns`), so a table with a minimal DDL is not broken by
stat columns it didn't ask for. User on-file values win over auto-injected
values when the keys collide.

Config-defined tables (`[[table]]` entries in `.dirsql.toml`) use a
synthesized `on_file` that returns one empty row per matched file; the
filesystem-fact layer fills it in. Programmatic tables (`Table::new` /
`Table::strict`) get the same auto-injection applied to whatever rows their
on_file callback returns.

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
5. The core merges glob captures and stat columns (filtered to the DDL's
   declared columns) into each returned row
6. Rows are inserted into SQLite with tracking metadata
7. File-to-rows mapping is stored for later diffing

### File change processing

1. `notify` detects a filesystem event (create/modify/delete)
2. The matcher checks if the file belongs to a table
3. For create/modify: `on_file` is called with the file's absolute path
   (reading the file itself if it needs the body), captures and stat columns
   are merged, `differ` compares old and new rows
4. For delete: old rows are retrieved, all emitted as delete events
5. SQLite is updated (old rows deleted, new rows inserted)
6. `RowEvent` objects are returned to Python

### Query execution

1. Python calls `db.query(sql)`
2. A SQLite authorizer is installed for the prepare, denying reads of the internal `_dirsql_*` bookkeeping tables (issue #378); a query touching one fails with a "not authorized" error
3. Rust executes the SQL against the ephemeral SQLite database
4. Results are converted from `HashMap<String, Value>` to Python dicts -- exactly the user's declared columns, no filtering (epic #358)
