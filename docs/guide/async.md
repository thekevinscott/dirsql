# Async API

`AsyncDirSQL` wraps the synchronous `DirSQL` to work with Python's `asyncio`. The initial directory scan runs in a background thread so it does not block the event loop.

## Basic usage

```python
import asyncio
import json
from dirsql import AsyncDirSQL, Table

async def main():
    db = AsyncDirSQL(
        "./my-project",
        tables=[
            Table(
                ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                glob="data/*.json",
                extract=lambda path, content: [json.loads(content)],
            ),
        ],
    )

    # Wait for the initial scan to complete
    await db.ready()

    # Query (runs in a thread, does not block the event loop)
    results = await db.query("SELECT * FROM items WHERE value > 10")
    print(results)

asyncio.run(main())
```

## Constructor

```python
AsyncDirSQL(root, *, tables, ignore=None)
```

The constructor takes the same arguments as `DirSQL`. It immediately starts scanning in a background thread via `asyncio.ensure_future`. The constructor itself returns immediately without blocking.

## `await db.ready()`

Waits until the initial directory scan is complete. If the scan raised an exception (invalid DDL, unreadable files, etc.), `ready()` re-raises it.

`ready()` can be called multiple times safely. After the first completion, subsequent calls return immediately.

```python
db = AsyncDirSQL("./data", tables=[...])

# Do other setup work here while the scan runs in the background
setup_logging()
connect_websocket()

# Now wait for the scan to finish
await db.ready()
```

## `await db.query(sql)`

Runs a SQL query in a background thread. Returns the same list-of-dicts format as the synchronous `DirSQL.query()`.

```python
results = await db.query("SELECT COUNT(*) as n FROM items")
```

## `async for event in db.watch()`

Returns an async iterable of `RowEvent` objects. The watcher is started automatically on the first iteration.

```python
async for event in db.watch():
    if event.action == "insert":
        print(f"New row in {event.table}: {event.row}")
    elif event.action == "update":
        print(f"Updated row in {event.table}: {event.row}")
    elif event.action == "delete":
        print(f"Deleted row from {event.table}: {event.row}")
    elif event.action == "error":
        print(f"Error: {event.error}")
```

The async iterator polls for filesystem events with a 200ms timeout internally. It yields events as they arrive and never terminates on its own -- use `break` or cancellation to stop.

## Combining with other async code

Because `AsyncDirSQL` uses `asyncio.to_thread` internally, it works alongside any other asyncio code without blocking:

```python
async def watch_and_serve(db):
    async for event in db.watch():
        await notify_clients(event)

async def main():
    db = AsyncDirSQL("./data", tables=[...])
    await db.ready()

    await asyncio.gather(
        watch_and_serve(db),
        run_web_server(),
    )
```
