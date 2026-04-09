# dirsql-core

Rust core library for `dirsql`. Provides the filesystem scanning, SQLite indexing, glob matching, file watching, and row diffing that power the language-specific SDKs.

## Installation

```bash
cargo add dirsql-core
```

## Modules

- **`db`** -- In-memory SQLite database management. Creates tables from DDL, inserts/updates/deletes rows, executes queries.
- **`scanner`** -- Walks a directory tree and matches files against glob patterns.
- **`matcher`** -- Glob pattern compilation and matching.
- **`watcher`** -- Filesystem event monitoring via the `notify` crate (inotify on Linux, FSEvents on macOS).
- **`differ`** -- Row-level diffing. Compares previous and current row sets for a file to produce insert/update/delete events.

## Usage

This crate is the engine layer consumed by the Rust SDK (`../rust/`), the Python SDK (`../python/`), and future SDKs. Direct Rust usage follows standard library patterns -- see the rustdoc for API details:

```bash
cargo doc --open -p dirsql-core
```

## License

MIT
