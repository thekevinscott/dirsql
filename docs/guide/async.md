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
                extract=lambda path: [json.loads(open(path, encoding="utf-8").read())],
            ),
        ],
    )
    await db.ready()

    # Query (runs in a thread, does not block the event loop)
    results = await db.query("SELECT * FROM items WHERE value > 10")
    print(results)

asyncio.run(main())
```

```rust [Rust]
use dirsql::{DirSQL, Table, Value};
use std::collections::HashMap;

// See `row_from_json` in getting-started.md for a reusable helper that
// turns a JSON object into a dirsql row (dirsql::Value is not Deserialize,
// so a row can't be produced by serde_json::from_str directly).
fn row_from_json(raw: &str) -> HashMap<String, Value> {
    let v: serde_json::Value = serde_json::from_str(raw).unwrap();
    let serde_json::Value::Object(obj) = v else { return HashMap::new() };
    obj.into_iter()
        .map(|(k, val)| {
            let v = match val {
                serde_json::Value::String(s) => Value::Text(s),
                serde_json::Value::Number(n) => n
                    .as_i64()
                    .map(Value::Integer)
                    .unwrap_or_else(|| Value::Real(n.as_f64().unwrap_or(0.0))),
                serde_json::Value::Bool(b) => Value::Integer(b as i64),
                serde_json::Value::Null => Value::Null,
                other => Value::Text(other.to_string()),
            };
            (k, v)
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DirSQL::new(
        "./my-project",
        vec![
            Table::new(
                "CREATE TABLE items (name TEXT, value INTEGER)",
                "data/*.json",
                |path| vec![row_from_json(&std::fs::read_to_string(path).unwrap())],
            ),
        ],
    )?;

    let results = db.query("SELECT * FROM items WHERE value > 10")?;
    println!("{:?}", results);
    Ok(())
}
```

```typescript [TypeScript]
import { readFileSync } from 'node:fs';
import { DirSQL, Table } from 'dirsql';

const db = new DirSQL({
  root: './my-project',
  tables: [
    new Table({
      ddl: 'CREATE TABLE items (name TEXT, value INTEGER)',
      glob: 'data/*.json',
      extract: (path) => [JSON.parse(readFileSync(path, 'utf8'))],
    }),
  ],
});

// Query
const results = await db.query('SELECT * FROM items WHERE value > 10');
console.log(results);
```

:::

## Constructor

```python
DirSQL(root=None, *, tables=None, ignore=None, config=None, persist=False, persist_path=None)
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
// `RowEvent` is an enum; match on the variant to destructure its fields.
// `StreamExt` (for `.next()`) comes from the `futures` crate, which is only a
// dirsql dependency under its `cli` feature -- add it to your own project:
//
//     cargo add futures
use dirsql::RowEvent;
use futures::StreamExt;

let mut stream = db.watch()?; // watch() returns Result<WatchStream>
while let Some(event) = stream.next().await {
    match event {
        RowEvent::Insert { table, row, file_path } => {
            println!("New row in {table} ({file_path}): {row:?}")
        }
        RowEvent::Update { table, new_row, file_path, .. } => {
            println!("Updated row in {table} ({file_path}): {new_row:?}")
        }
        RowEvent::Delete { table, row, file_path } => {
            println!("Deleted row from {table} ({file_path}): {row:?}")
        }
        RowEvent::Error { file_path, error, .. } => {
            eprintln!("Error on {file_path:?}: {error}")
        }
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
// `.next()` needs `StreamExt` from the `futures` crate (`cargo add futures`).
use futures::StreamExt;

async fn watch_and_serve(db: &DirSQL) -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = db.watch()?; // watch() returns Result<WatchStream>
    while let Some(event) = stream.next().await {
        notify_clients(&event).await;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DirSQL::new("./data", vec![/* tables */])?;

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

const db = new DirSQL({ root: './data', tables: [/* tables */] });

await Promise.all([
  watchAndServe(db),
  runWebServer(),
]);
```

:::
