# File Watching

`dirsql` can monitor the filesystem for changes and emit events when rows are inserted, updated, or deleted. This is useful for building reactive applications that respond to file changes in real time.

## Starting a watch stream

::: code-group

```python [Python]
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "./my-project",
    tables=[
        Table(
            ddl="CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
            glob="comments/**/*.json",
            extract=lambda path, content: [json.loads(content)],
        ),
    ],
)

async for event in db.watch():
    print(f"{event.action} on {event.table}: {event.row}")
```

```rust [Rust]
use dirsql_sdk::{DirSQL, Table};
use futures::StreamExt;

let db = DirSQL::new(
    "./my-project",
    vec![
        Table::new(
            "CREATE TABLE comments (id TEXT, body TEXT, author TEXT)",
            "comments/**/*.json",
            |_path, content| vec![serde_json::from_str(content).unwrap()],
        ),
    ],
)?;

let mut stream = db.watch();
while let Some(event) = stream.next().await {
    println!("{} on {}: {:?}", event.action, event.table, event.row);
}
```

```typescript [TypeScript]
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL('./my-project', {
  tables: [
    new Table({
      ddl: 'CREATE TABLE comments (id TEXT, body TEXT, author TEXT)',
      glob: 'comments/**/*.json',
      extract: (_path, content) => [JSON.parse(content)],
    }),
  ],
});

for await (const event of db.watch()) {
  console.log(`${event.action} on ${event.table}:`, event.row);
}
```

:::

See [Async API](./async.md) for full details on the async `DirSQL` API (Python).

## Event types

Each event is a `RowEvent` object with these attributes:

### `insert`

A new row was added. This happens when a new file is created or an existing file gains additional rows.

::: code-group

```python [Python]
event.action   # "insert"
event.table    # "comments"
event.row      # {"id": "abc", "body": "new comment", "author": "alice"}
event.old_row  # None
event.file_path # "comments/abc/index.json"
```

```rust [Rust]
event.action   // Action::Insert
event.table    // "comments"
event.row      // Some({"id": "abc", "body": "new comment", "author": "alice"})
event.old_row  // None
event.file_path // "comments/abc/index.json"
```

```typescript [TypeScript]
event.action   // 'insert'
event.table    // 'comments'
event.row      // { id: 'abc', body: 'new comment', author: 'alice' }
event.oldRow   // undefined
event.filePath // 'comments/abc/index.json'
```

:::

### `update`

An existing row was modified. `dirsql` diffs the old and new rows extracted from the file to detect changes.

::: code-group

```python [Python]
event.action   # "update"
event.table    # "comments"
event.row      # {"id": "abc", "body": "edited comment", "author": "alice"}
event.old_row  # {"id": "abc", "body": "original comment", "author": "alice"}
event.file_path # "comments/abc/index.json"
```

```rust [Rust]
event.action   // Action::Update
event.table    // "comments"
event.row      // Some({"id": "abc", "body": "edited comment", "author": "alice"})
event.old_row  // Some({"id": "abc", "body": "original comment", "author": "alice"})
event.file_path // "comments/abc/index.json"
```

```typescript [TypeScript]
event.action   // 'update'
event.table    // 'comments'
event.row      // { id: 'abc', body: 'edited comment', author: 'alice' }
event.oldRow   // { id: 'abc', body: 'original comment', author: 'alice' }
event.filePath // 'comments/abc/index.json'
```

:::

### `delete`

A row was removed. This happens when a file is deleted or a file is modified to contain fewer rows.

::: code-group

```python [Python]
event.action   # "delete"
event.table    # "comments"
event.row      # {"id": "abc", "body": "deleted comment", "author": "alice"}
event.old_row  # None
event.file_path # "comments/abc/index.json"
```

```rust [Rust]
event.action   // Action::Delete
event.table    // "comments"
event.row      // Some({"id": "abc", "body": "deleted comment", "author": "alice"})
event.old_row  // None
event.file_path // "comments/abc/index.json"
```

```typescript [TypeScript]
event.action   // 'delete'
event.table    // 'comments'
event.row      // { id: 'abc', body: 'deleted comment', author: 'alice' }
event.oldRow   // undefined
event.filePath // 'comments/abc/index.json'
```

:::

### `error`

An error occurred while processing a file change. The file was modified but the extract function failed, or the file could not be read.

::: code-group

```python [Python]
event.action    # "error"
event.table     # "comments"
event.error     # "Extract error: ..."
event.file_path # "comments/abc/index.json"
event.row       # None
```

```rust [Rust]
event.action    // Action::Error
event.table     // "comments"
event.error     // Some("Extract error: ...")
event.file_path // "comments/abc/index.json"
event.row       // None
```

```typescript [TypeScript]
event.action   // 'error'
event.table    // 'comments'
event.error    // 'Extract error: ...'
event.filePath // 'comments/abc/index.json'
event.row      // undefined
```

:::

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
