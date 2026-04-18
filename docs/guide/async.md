# Async API

`DirSQL` is async by default in Python. The initial directory scan runs in a background thread so it does not block the event loop.

## Basic usage

::: code-group

```python [Python]
import asyncio
import json
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "./my-project",
        tables=[
            Table(
                ddl="CREATE TABLE items (name TEXT, value INTEGER)",
                glob="data/*.json",
                extract=lambda path, content: [json.loads(content)],
            ),
        ],
    )

    # Query (runs in a thread, does not block the event loop)
    results = await db.query("SELECT * FROM items WHERE value > 10")
    print(results)

asyncio.run(main())
```

```rust [Rust]
use dirsql::{DirSQL, Table};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DirSQL::new(
        "./my-project",
        vec![
            Table::new(
                "CREATE TABLE items (name TEXT, value INTEGER)",
                "data/*.json",
                |_path, content| vec![serde_json::from_str(content).unwrap()],
            ),
        ],
    )?;

    let results = db.query("SELECT * FROM items WHERE value > 10")?;
    println!("{:?}", results);
    Ok(())
}
```

```typescript [TypeScript]
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL('./my-project', [
  new Table({
    ddl: 'CREATE TABLE items (name TEXT, value INTEGER)',
    glob: 'data/*.json',
    extract: (_path, content) => [JSON.parse(content)],
  }),
]);

// `query()` is synchronous in TypeScript (there is no AsyncDirSQL
// class — see PARITY.md). The call blocks the JS thread for the
// duration of the SQLite read.
const results = db.query('SELECT * FROM items WHERE value > 10');
console.log(results);
```

:::

## Constructor

```python
DirSQL(root, *, tables, ignore=None)
```

The constructor immediately starts scanning in a background thread via `asyncio.ensure_future`. The constructor itself returns immediately without blocking.

## `await db.ready()`

Waits until the initial directory scan is complete. If the scan raised an exception (invalid DDL, unreadable files, etc.), `ready()` re-raises it.

`ready()` can be called multiple times safely. After the first completion, subsequent calls return immediately.

```python
db = DirSQL("./data", tables=[...])

# Do other setup work here while the scan runs in the background
setup_logging()
connect_websocket()

# Now wait for the scan to finish before querying
await db.ready()
```

## `await db.query(sql)`

Runs a SQL query in a background thread. Returns a list of dicts keyed by column name.

```python
results = await db.query("SELECT COUNT(*) as n FROM items")
```

## `async for event in db.watch()`

Returns an async iterable of `RowEvent` objects. The watcher is started automatically on the first iteration.

::: code-group

```python [Python]
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

```rust [Rust]
use futures::StreamExt;

let mut stream = db.watch();
while let Some(event) = stream.next().await {
    match event.action {
        Action::Insert => println!("New row in {}: {:?}", event.table, event.row),
        Action::Update => println!("Updated row in {}: {:?}", event.table, event.row),
        Action::Delete => println!("Deleted row from {}: {:?}", event.table, event.row),
        Action::Error => eprintln!("Error: {:?}", event.error),
    }
}
```

```typescript [TypeScript]
for await (const event of db.watch()) {
  switch (event.action) {
    case 'insert':
      console.log(`New row in ${event.table}:`, event.row);
      break;
    case 'update':
      console.log(`Updated row in ${event.table}:`, event.row);
      break;
    case 'delete':
      console.log(`Deleted row from ${event.table}:`, event.row);
      break;
    case 'error':
      console.error(`Error: ${event.error}`);
      break;
  }
}
```

:::

The async iterator polls for filesystem events with a 200ms timeout internally. It yields events as they arrive and never terminates on its own -- use `break` or cancellation to stop.

## Combining with other async code

The async API works alongside other concurrent code without blocking:

::: code-group

```python [Python]
async def watch_and_serve(db):
    async for event in db.watch():
        await notify_clients(event)

async def main():
    db = DirSQL("./data", tables=[...])
    await asyncio.gather(
        watch_and_serve(db),
        run_web_server(),
    )
```

```rust [Rust]
async fn watch_and_serve(db: &DirSQL) {
    let mut stream = db.watch();
    while let Some(event) = stream.next().await {
        notify_clients(&event).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DirSQL::new("./data", vec![...])?;

    tokio::join!(
        watch_and_serve(&db),
        run_web_server(),
    );
    Ok(())
}
```

```typescript [TypeScript]
async function watchAndServe(db: DirSQL) {
  for await (const event of db.watch()) {
    await notifyClients(event);
  }
}

const db = new DirSQL('./data', [/* tables */]);

await Promise.all([
  watchAndServe(db),
  runWebServer(),
]);
```

:::
