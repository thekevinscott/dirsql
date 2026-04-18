# `dirsql` (Python SDK)

Ephemeral SQL index over a local directory. Watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface. The database is purely in-memory -- the filesystem is always the source of truth.

[Documentation](https://thekevinscott.github.io/dirsql/?lang=python)

## Installation

```bash
pip install dirsql
```

Requires Python >= 3.12. Ships as a native extension (Rust via PyO3) -- binary wheels are provided for common platforms.

Each wheel also bundles the `dirsql` HTTP-server CLI as a console script, so `pip install dirsql` also gives you a `dirsql` command on `$PATH`. See the [CLI guide](https://github.com/thekevinscott/dirsql/blob/main/docs/guide/cli.md).

## Publishing (maintainers)

Handled by `.github/workflows/publish.yml` (invoked from `minor-release.yml` / `patch-release.yml`). For each target triple the `build` job `cargo build`s the Rust CLI with `--features cli`, stages the binary into `python/dirsql/_binary/`, runs `maturin build` (which picks the binary up via the `[tool.maturin] include` rule in `pyproject.toml`), and the wheels + sdist are then trusted-published to PyPI.

## Quick Start

```python
import asyncio
import json
import os
import tempfile
from dirsql import DirSQL, Table

async def main():
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
    await db.ready()

    # Query with SQL
    results = await db.query("SELECT * FROM comments WHERE author = 'alice'")
    # [{"id": "abc", "body": "looks good", "author": "alice"}]

asyncio.run(main())
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
await db.ready()

results = await db.query("""
    SELECT posts.title, authors.name
    FROM posts JOIN authors ON posts.author_id = authors.id
""")
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

## Watching for Changes

`DirSQL` is async by default. The `watch()` method returns an async iterator of row-level change events.

```python
import asyncio
import json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "/path/to/data",
        tables=[
            Table(
                ddl="CREATE TABLE items (name TEXT)",
                glob="**/*.json",
                extract=lambda path, content: [json.loads(content)],
            ),
        ],
    )
    await db.ready()

    # Query
    results = await db.query("SELECT * FROM items")

    # Watch for file changes (insert/update/delete/error events)
    async for event in db.watch():
        print(f"{event.action} on {event.table}: {event.row}")
        if event.action == "error":
            print(f"  error: {event.error}")

asyncio.run(main())
```

## API Reference

### `Table(*, ddl, glob, extract)`

Defines how files map to a SQL table.

- **`ddl`** (`str`): A `CREATE TABLE` statement defining the schema.
- **`glob`** (`str`): A glob pattern matched against file paths relative to root.
- **`extract`** (`Callable[[str, str], list[dict]]`): A function receiving `(relative_path, file_content)` and returning a list of row dicts. Each dict's keys must match the DDL column names.

### `DirSQL(root=None, *, tables=None, ignore=None, config=None)`

Creates an in-memory SQLite database indexed from the directory at `root`. The constructor is sync and returns immediately; scanning runs in a background thread.

At least one of `root` or `config` must be supplied. When both `root` and `config` are passed (or `config` declares `[dirsql].root`), the explicit `root` wins and a warning is emitted on stderr.

- **`root`** (`str | None`): Path to the directory to index. Optional when `config` supplies one.
- **`tables`** (`list[Table] | None`): Programmatic table definitions. Appended to any tables in the config file.
- **`ignore`** (`list[str] | None`): Glob patterns for paths to skip. Appended to any `[dirsql].ignore` patterns in the config file.
- **`config`** (`str | None`): Optional path to a `.dirsql.toml` file. Its `[[table]]` entries, `[dirsql].ignore`, and optional `[dirsql].root` are merged into the constructor's inputs.

#### `await DirSQL.ready()`

Wait for the initial scan to complete. Idempotent -- safe to call multiple times. Raises any exception that occurred during init.

#### `await DirSQL.query(sql) -> list[dict]`

Execute a SQL query. Returns a list of dicts keyed by column name. Internal tracking columns (`_dirsql_*`) are excluded from results.

#### `DirSQL.watch() -> AsyncIterator[RowEvent]`

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

## License

MIT
