# `dirsql` (Python SDK)

Ephemeral SQL index over a local directory. `dirsql` watches a filesystem, ingests structured files into an in-memory SQLite database, and exposes a SQL query interface -- the filesystem is always the source of truth.

[Documentation](https://dirsql.dev/?lang=python)

Also available as [`dirsql` on crates.io](https://crates.io/crates/dirsql) and [`dirsql` on npm](https://www.npmjs.com/package/dirsql).

## Installation

```bash
pip install dirsql
```

Requires Python >= 3.12. Ships as a native extension (Rust via PyO3); prebuilt binary wheels are provided for common platforms.

## Quick start

`DirSQL` is async by default: the constructor returns immediately, scanning runs in a background thread, and you `await db.ready()` before querying. Each table is a `name`, a `ddl`, a `glob` and an `on_file` callback: the name is the table's SQL name, the DDL defines the SQLite schema and must create a table by that name, the glob selects files (relative to the root), and `on_file` turns a matched file into a list of row dicts. `dirsql` does not read file contents -- if `on_file` needs the file body it reads `path` itself; return an empty list to skip a file.

```python
import asyncio
import json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "./my-blog",
        tables=[
            Table(
                name="posts",
                ddl="CREATE TABLE posts (title TEXT, author TEXT)",
                glob="posts/*.json",
                on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
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
            name="posts",
            ddl="CREATE TABLE posts (title TEXT, author_id TEXT)",
            glob="posts/*.json",
            on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
        ),
        Table(
            name="authors",
            ddl="CREATE TABLE authors (id TEXT, name TEXT)",
            glob="authors/*.json",
            on_file=lambda path: [json.loads(open(path, encoding="utf-8").read())],
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

Path-table scans (`SELECT ... FROM './'`) respect `.gitignore` files by
default. Pass `no_ignore=True` to restore the full walk; the built-in
`.git` default and any `ignore` patterns still apply:

```python
db = DirSQL("./my-blog", no_ignore=True)
```

## Loading SQLite extensions

Pass `extensions` to load SQLite extension shared libraries onto the connection at startup (before any `CREATE TABLE`). Each entry is a dict with a `path` and an optional `entrypoint` init-symbol override:

```python
db = DirSQL(
    "./my-blog",
    tables=[...],
    extensions=[
        {"path": "./ext/vec0.dylib", "entrypoint": "sqlite3_vec_init"},
        {"path": "./ext/myext.so"},  # entrypoint derived from the filename
    ],
)
await db.ready()

# The extension's functions are now callable in queries:
rows = await db.query("SELECT vec_version() AS v")
```

`dirsql` enables extension loading only while loading the configured libraries, then disables it again, so the SQL `load_extension()` function is never exposed to your queries. Programmatic entries load first, followed by any `[[dirsql.extension]]` entries declared in a `config` file. See the [config reference](https://github.com/thekevinscott/dirsql/blob/main/docs/reference/config.md#dirsqlextension).

## Watching for changes

`db.watch()` returns an async iterator of row-level change events as files change on disk:

```python
async with db.watch() as events:
    async for event in events:
        print(f"{event.action} on {event.table}: {event.row}")
        if event.action == "error":
            print(f"  error: {event.error}")
```

The `async with` block closes the stream on exit, which releases the events it has not returned yet; `await events.aclose()` does the same directly. `async for event in db.watch():` also works, and that stream is released when Python collects it.

Each event has `.action` (`"insert"`, `"update"`, `"delete"`, or `"error"`), `.table`, `.row` (the new row, or the deleted row on `delete`), `.old_row` (the previous row, on `update`), `.file_path`, and `.error` (on `error`).

## CLI

`pip install dirsql` also installs a `dirsql` console script. `dirsql "<sql>"` (or `uvx dirsql "<sql>"`) runs one query and prints the rows as JSON — the default. `dirsql server` starts an HTTP server exposing the SDK over HTTP: `POST /query` for SQL and `GET /events` for a Server-Sent Events change stream. See the [CLI reference](https://dirsql.dev/reference/cli).

## License

MIT
