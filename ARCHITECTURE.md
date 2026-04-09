# Architecture

## Core Principle: One Implementation, Thin Bindings

**The Rust core (`packages/core/`) is the single source of truth for all business logic.** Every language SDK is a thin binding layer that wraps the core -- it does NOT reimplement it.

- **`packages/core/`** -- `dirsql-core` Rust crate. All business logic lives here: SQLite operations, glob matching, file scanning, row diffing, file watching.
- **`packages/python/`** -- PyO3 bindings wrapping `dirsql-core`. Thin glue code + async Python wrapper.
- **`packages/rust/`** -- Ergonomic Rust SDK wrapping `dirsql-core`. Builder pattern, async support via tokio.
- **`packages/ts/`** -- napi-rs bindings wrapping `dirsql-core`. (Not yet implemented.)

**Never reimplement core logic in a language SDK.** If you're writing SQLite operations, glob matching, file scanning, or row diffing in Python or TypeScript, that code belongs in the Rust core with a binding exposed to the SDK. The entire point of this architecture is a fast Rust core with language bindings, not three independent implementations.

## Cross-Language Parity

Aim for **complete API parity across all three SDKs**: same concepts, same capabilities, same naming where possible. Exceptions are allowed for language-idiomatic patterns:

- **Python**: `await db.ready()` (method call). snake_case. Async iterators for event streams.
- **TypeScript**: `await db.ready` (awaitable property). camelCase. AsyncIterables for event streams.
- **Rust**: Builder pattern or `db.ready().await`. snake_case. Stream trait for event streams.

When adding a feature to one SDK, create beads for the other two.

## Overview

`dirsql` is a Rust core with language-specific SDK wrappers.

```
┌─────────────────────────────────┐
│         Python SDK              │
│   DirSQL, AsyncDirSQL, Table    │
├─────────────────────────────────┤
│         PyO3 bindings           │
│   packages/python/src/lib.rs    │
├─────────────────────────────────┤
│         Rust core               │
│   packages/core/src/            │
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

## Rust core (`packages/core/`)

The core library is a Rust crate (`dirsql-core`) that handles all heavy lifting:

### `db` -- In-memory SQLite

Wraps `rusqlite` with an in-memory database. Handles DDL execution, row insertion with internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`), querying with automatic exclusion of tracking columns, and row deletion by file path.

### `scanner` -- Directory traversal

Walks a directory tree and matches files against table globs. Returns a list of `(file_path, table_name)` pairs. Uses the `matcher` module internally.

### `matcher` -- Glob-to-table mapping

Maps glob patterns to table names and handles ignore patterns. A file is matched against globs in registration order; the first match wins.

### `watcher` -- Filesystem monitoring

Wraps the `notify` crate to watch for filesystem changes. Emits `FileEvent` variants: `Created`, `Modified`, `Deleted`. Uses a channel-based architecture where events are sent from a background thread and received via `recv_timeout` and `try_recv_all`.

### `differ` -- Row diffing

Compares old and new row sets for a file to produce `RowEvent` variants: `Insert`, `Update`, `Delete`, `Error`. Rows are compared by position (index within the file).

## Python SDK (`packages/python/`)

### PyO3 bindings

The `lib.rs` file in `packages/python/src/` defines the PyO3 bindings that expose the Rust core to Python:

- `Table` (PyO3 class) -- stores DDL, glob, and the Python extract callable
- `DirSQL` (PyO3 class) -- owns the database, table configs, file-row tracking, and watcher
- `RowEvent` (PyO3 class) -- represents a row-level change event

The Python `extract` callable is called from Rust via PyO3's GIL-acquiring mechanism. Python dicts are converted to `HashMap<String, Value>` for storage, and converted back for query results.

### AsyncDirSQL

A pure-Python wrapper (`_async.py`) that uses `asyncio.to_thread` to run the synchronous Rust operations off the event loop. The watch stream is implemented as a custom async iterator that polls for events in a background thread.

## Data flow

### Startup scan

1. Python creates `DirSQL` with root path and table definitions
2. Rust executes DDL to create SQLite tables
3. `scanner` walks the directory and matches files to tables
4. For each matched file, Python `extract` is called via PyO3
5. Extracted rows are inserted into SQLite with tracking metadata
6. File-to-rows mapping is stored for later diffing

### File change processing

1. `notify` detects a filesystem event (create/modify/delete)
2. The matcher checks if the file belongs to a table
3. For create/modify: file is re-read, `extract` is called, `differ` compares old and new rows
4. For delete: old rows are retrieved, all emitted as delete events
5. SQLite is updated (old rows deleted, new rows inserted)
6. `RowEvent` objects are returned to Python

### Query execution

1. Python calls `db.query(sql)`
2. Rust executes the SQL against in-memory SQLite
3. Results are converted from `HashMap<String, Value>` to Python dicts
4. Internal `_dirsql_*` columns are filtered out before returning
