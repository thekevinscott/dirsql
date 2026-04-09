# File Watching

`dirsql` can monitor the filesystem for changes and emit events when rows are inserted, updated, or deleted. This is useful for building reactive applications that respond to file changes in real time.

## Starting a watch stream

Use the synchronous `DirSQL` with the async watch API:

```python
from dirsql import AsyncDirSQL, Table
import json

db = AsyncDirSQL(
    "./my-project",
    tables=[
        Table(
            ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
            glob="comments/**/*.jsonl",
            extract=lambda path, content: [
                json.loads(line) for line in content.splitlines()
            ],
        ),
    ],
)

await db.ready()

async for event in db.watch():
    print(f"{event.action} on {event.table}: {event.row}")
```

See [Async API](./async.md) for full details on `AsyncDirSQL`.

## Event types

Each event is a `RowEvent` object with these attributes:

### `insert`

A new row was added. This happens when a new file is created or an existing file gains additional rows.

```python
event.action   # "insert"
event.table    # "comments"
event.row      # {"id": "abc", "body": "new comment", "author": "alice"}
event.old_row  # None
event.file_path # "comments/abc/index.jsonl"
```

### `update`

An existing row was modified. `dirsql` diffs the old and new rows extracted from the file to detect changes.

```python
event.action   # "update"
event.table    # "comments"
event.row      # {"id": "abc", "body": "edited comment", "author": "alice"}
event.old_row  # {"id": "abc", "body": "original comment", "author": "alice"}
event.file_path # "comments/abc/index.jsonl"
```

### `delete`

A row was removed. This happens when a file is deleted or a file is modified to contain fewer rows.

```python
event.action   # "delete"
event.table    # "comments"
event.row      # {"id": "abc", "body": "deleted comment", "author": "alice"}
event.old_row  # None
event.file_path # "comments/abc/index.jsonl"
```

### `error`

An error occurred while processing a file change. The file was modified but the extract function failed, or the file could not be read.

```python
event.action    # "error"
event.table     # "comments"
event.error     # "Extract error: ..."
event.file_path # "comments/abc/index.jsonl"
event.row       # None
```

## How diffing works

When a file changes, `dirsql`:

1. Re-reads the file and calls the extract function to get new rows
2. Compares new rows against the previously extracted rows for that file
3. Emits insert, update, and delete events based on the diff
4. Updates the in-memory database to reflect the new state

Row identity is determined by position (row index within the file). If a file previously produced 3 rows and now produces 2, the first two rows are compared for updates and the third is emitted as a delete.

## Filesystem events

Under the hood, `dirsql` uses the `notify` crate (inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows) to receive filesystem events. Events are coalesced and filtered through the table matcher before being processed.

Files that do not match any table glob or that match an ignore pattern are silently skipped.
