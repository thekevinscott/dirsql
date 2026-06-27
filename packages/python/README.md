# `dirsql` (Python SDK)

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface -- the filesystem is always the source of truth.

[Documentation](https://thekevinscott.github.io/dirsql/?lang=python)

Also available as [`dirsql` on crates.io](https://crates.io/crates/dirsql) and [`dirsql` on npm](https://www.npmjs.com/package/dirsql).

## Installation

```bash
pip install dirsql
```

Requires Python >= 3.12. Ships as a native extension (Rust via PyO3); prebuilt binary wheels are provided for common platforms.

## Quick start

`DirSQL` is async by default: the constructor returns immediately, scanning runs in a background thread, and you `await db.ready()` before querying. Each table is a `(ddl, glob, extract)` triple: the DDL defines the SQLite schema, the glob selects files (relative to the root), and `extract` turns a matched file into a list of row dicts. `dirsql` does not read file contents -- if `extract` needs the file body it reads `path` itself; return an empty list to skip a file.

```python
import asyncio
import json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "./my-blog",
        tables=[
            Table(
                ddl="CREATE TABLE posts (title TEXT, author TEXT)",
                glob="posts/*.json",
                extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
        ],
    )
    await db.ready()

    posts = await db.query("SELECT * FROM posts WHERE author = 'alice'")
    print(posts)

asyncio.run(main())
```

## Multiple tables and joins

```python
db = DirSQL(
    "./my-blog",
    tables=[
        Table(
            ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
            glob="posts/*.json",
            extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
        ),
        Table(
            ddl="CREATE TABLE authors (id TEXT, name TEXT)",
            glob="authors/*.json",
            extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
        ),
    ],
)
await db.ready()

results = await db.query("""
    SELECT posts.title, authors.name
    FROM posts JOIN authors ON posts.author_id = authors.id
""")
```

## Ignoring files

Pass `ignore` patterns to skip files during scanning and watching:

```python
db = DirSQL(
    "./my-blog",
    ignore=["**/drafts/**", "**/.git/**"],
    tables=[...],
)
```

## Watching for changes

`db.watch()` returns an async iterator of row-level change events as files change on disk:

```python
async for event in db.watch():
    print(f"{event.action} on {event.table}: {event.row}")
    if event.action == "error":
        print(f"  error: {event.error}")
```

Each event has `.action` (`"insert"`, `"update"`, `"delete"`, or `"error"`), `.table`, `.row` (the new row, or the deleted row on `delete`), `.old_row` (the previous row, on `update`), `.file_path`, and `.error` (on `error`).

## CLI

`pip install dirsql` also installs a `dirsql` console script that runs an HTTP server exposing the SDK over HTTP: `POST /query` for SQL and `GET /events` for a Server-Sent Events change stream. Run `dirsql` (or `uvx dirsql`) to start it. See the [CLI guide](https://thekevinscott.github.io/dirsql/cli/).

## License

MIT
