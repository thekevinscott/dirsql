# dirsql

Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. On shutdown, the database is discarded -- the filesystem remains the source of truth.

## Why

Structured data stored as flat files (JSONL, JSON) is easy to read, write, diff, and version. But querying across many files is slow -- "show me all unresolved comments across 50 documents" requires opening and parsing every file.

dirsql bridges this gap: files remain the source of truth, but you get SQL queries and real-time change events for free.

## Installation

```bash
pip install dirsql
```

Rust (library only, no Python bindings):

```bash
cargo add dirsql
```

## Quick Start

```python
import json
import os
import tempfile
from dirsql import DirSQL, Table

# Create some data files
root = tempfile.mkdtemp()
os.makedirs(os.path.join(root, "comments", "abc"), exist_ok=True)
os.makedirs(os.path.join(root, "comments", "def"), exist_ok=True)

with open(os.path.join(root, "comments", "abc", "index.jsonl"), "w") as f:
    f.write(json.dumps({"body": "looks good", "author": "alice"}) + "\n")
    f.write(json.dumps({"body": "needs work", "author": "bob"}) + "\n")

with open(os.path.join(root, "comments", "def", "index.jsonl"), "w") as f:
    f.write(json.dumps({"body": "agreed", "author": "carol"}) + "\n")

# Define a table: DDL, glob pattern, and an extract function
db = DirSQL(
    root,
    tables=[
        Table(
            ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
            glob="comments/**/index.jsonl",
            extract=lambda path, content: [
                {
                    "id": os.path.basename(os.path.dirname(path)),
                    "body": row["body"],
                    "author": row["author"],
                }
                for line in content.splitlines()
                for row in [json.loads(line)]
            ],
        ),
    ],
)

# Query with SQL
results = db.query("SELECT * FROM comments WHERE author = 'alice'")
# [{"id": "abc", "body": "looks good", "author": "alice"}]
```

## Multiple Tables and Joins

```python
db = DirSQL(
    root,
    tables=[
        Table(
            ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
            glob="posts/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
        Table(
            ddl="CREATE TABLE authors (id TEXT, name TEXT)",
            glob="authors/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ],
)

results = db.query("""
    SELECT posts.title, authors.name
    FROM posts JOIN authors ON posts.author_id = authors.id
""")
```

## Async API

`AsyncDirSQL` wraps the synchronous API for use with asyncio. Initialization is awaitable, and `watch()` returns an async iterator of row-level change events.

```python
import asyncio
import json
import os
from dirsql import AsyncDirSQL, Table

async def main():
    db = await AsyncDirSQL(
        "/path/to/data",
        tables=[
            Table(
                ddl="CREATE TABLE items (name TEXT)",
                glob="**/*.json",
                extract=lambda path, content: [json.loads(content)],
            ),
        ],
    )

    # Query works the same way
    results = await db.query("SELECT * FROM items")

    # Watch for file changes (insert/update/delete/error events)
    async for event in db.watch():
        print(f"{event.action} on {event.table}: {event.row}")
        if event.action == "error":
            print(f"  error: {event.error}")

asyncio.run(main())
```

## Ignoring Files

Pass `ignore` patterns to skip files during scanning and watching:

```python
db = DirSQL(
    root,
    ignore=["**/drafts/**", "**/.git/**"],
    tables=[...],
)
```

## API Reference

### `Table(*, ddl, glob, extract)`

Defines how files map to a SQL table.

- **`ddl`** (`str`): A `CREATE TABLE` statement defining the schema.
- **`glob`** (`str`): A glob pattern matched against file paths relative to root.
- **`extract`** (`Callable[[str, str], list[dict]]`): A function receiving `(relative_path, file_content)` and returning a list of row dicts. Each dict's keys must match the DDL column names.

### `DirSQL(root, *, tables, ignore=None)`

Creates an in-memory SQLite database indexed from the directory at `root`.

- **`root`** (`str`): Path to the directory to index.
- **`tables`** (`list[Table]`): Table definitions.
- **`ignore`** (`list[str] | None`): Glob patterns for paths to skip.

#### `DirSQL.query(sql) -> list[dict]`

Execute a SQL query. Returns a list of dicts keyed by column name. Internal tracking columns (`_dirsql_*`) are excluded from results.

### `AsyncDirSQL(root, *, tables, ignore=None)`

Async wrapper. Must be `await`ed to initialize.

#### `await AsyncDirSQL.query(sql) -> list[dict]`

Same as `DirSQL.query`, but async.

#### `AsyncDirSQL.watch() -> AsyncIterator[RowEvent]`

Returns an async iterator that yields `RowEvent` objects as files change on disk. Starts the filesystem watcher on first iteration.

### `RowEvent`

Emitted by `watch()` when a file change produces row-level diffs.

- **`table`** (`str`): The affected table name.
- **`action`** (`str`): One of `"insert"`, `"update"`, `"delete"`, `"error"`.
- **`row`** (`dict | None`): The new row (for insert/update) or deleted row (for delete).
- **`old_row`** (`dict | None`): The previous row (for update only).
- **`error`** (`str | None`): Error message (for error events).
- **`file_path`** (`str | None`): The relative file path that triggered the event.

## How It Works

The Rust core (`rusqlite` + `notify` + `walkdir`) does the heavy lifting:

1. **Startup scan**: Walks the directory tree, matches files to tables via glob patterns, calls the user-provided `extract` function for each file, and inserts rows into an in-memory SQLite database.
2. **File watching**: Uses the `notify` crate (inotify on Linux, FSEvents on macOS) to detect file creates, modifications, and deletions.
3. **Row diffing**: When a file changes, the new rows are diffed against the previous rows for that file, producing granular insert/update/delete events.
4. **Python bindings**: PyO3 exposes the Rust core as a native Python extension module. The async layer runs blocking operations in a thread pool via `asyncio.to_thread`.

The SQLite database is purely ephemeral -- it exists only in memory and is discarded when the `DirSQL` instance is garbage collected. The filesystem is always the source of truth.

## Development

### Prerequisites

- Rust (stable)
- Python >= 3.12
- [maturin](https://github.com/PyO3/maturin) for building the Python extension
- [just](https://github.com/casey/just) as a task runner

### Build and Test

```bash
# Build the Python extension (dev mode)
maturin develop

# Run all CI checks
just ci

# Individual targets
just test-rust        # Rust unit tests
just test-integration # Python integration tests
just clippy           # Rust lints
just lint             # Python lints (ruff)
```

## License

MIT
