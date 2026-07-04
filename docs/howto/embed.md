# Embed `dirsql` in your application

Run the same engine in-process instead of as a server: construct a `DirSQL`
over a directory, hand it your tables, and query it like any library — no
HTTP, no separate process. The SDKs for Python, TypeScript, and Rust are
thin bindings over one shared core, so behavior matches the CLI exactly.

## 1. Install the SDK

::: code-group

```bash [Python]
uv add dirsql
```

```bash [TypeScript]
pnpm add dirsql
```

```bash [Rust]
cargo add dirsql
```

:::

## 2. Index a directory and query it

Suppose comments live as JSON files filed by thread:

```
comments/t1/c1.json    # {"body": "first!", "author": "alice"}
comments/t1/c2.json    # {"body": "nice post", "author": "bob"}
comments/t2/c1.json    # {"body": "following up", "author": "alice"}
```

Unlike a config-file table, a programmatic table takes an `extract`
callback — your code reads each matched file and returns its rows, with
[glob captures and virtual columns](../reference/columns.md) merged on
automatically (here, `{thread}` from the path):

::: code-group

```python [Python]
import asyncio
import json

from dirsql import DirSQL, Table


def extract(path: str) -> list[dict]:
    with open(path, encoding="utf-8") as f:
        return [json.load(f)]


async def main() -> None:
    db = DirSQL(
        "./comments-root",
        tables=[
            Table(
                ddl="CREATE TABLE comments (thread TEXT, author TEXT, body TEXT)",
                glob="comments/{thread}/*.json",
                extract=extract,
            )
        ],
    )
    rows = await db.query("SELECT thread, author, body FROM comments ORDER BY thread, author")
    for row in rows:
        print(f"[{row['thread']}] {row['author']}: {row['body']}")


asyncio.run(main())
```

```typescript [TypeScript]
import { readFileSync } from "node:fs";
import { DirSQL } from "dirsql";

const db = new DirSQL({
  root: "./comments-root",
  tables: [
    {
      ddl: "CREATE TABLE comments (thread TEXT, author TEXT, body TEXT)",
      glob: "comments/{thread}/*.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});

const rows = await db.query(
  "SELECT thread, author, body FROM comments ORDER BY thread, author",
);
for (const row of rows) {
  console.log(`[${row.thread}] ${row.author}: ${row.body}`);
}
```

```rust [Rust]
use std::collections::HashMap;

use dirsql::{DirSQL, Table, Value};

fn extract(path: &str) -> Vec<HashMap<String, Value>> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(&raw) else {
        return vec![];
    };
    let mut row = HashMap::new();
    for (key, value) in obj {
        if let serde_json::Value::String(s) = value {
            row.insert(key, Value::Text(s));
        }
    }
    vec![row]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = DirSQL::builder()
        .root("./comments-root")
        .table(Table::new(
            "CREATE TABLE comments (thread TEXT, author TEXT, body TEXT)",
            "comments/{thread}/*.json",
            extract,
        ))
        .build()?;

    let rows = db.query("SELECT thread, author, body FROM comments ORDER BY thread, author")?;
    for row in rows {
        if let (Value::Text(thread), Value::Text(author), Value::Text(body)) =
            (&row["thread"], &row["author"], &row["body"])
        {
            println!("[{thread}] {author}: {body}");
        }
    }
    Ok(())
}
```

:::

Rows come back keyed by column name (a `dict` / object /
`HashMap<String, Value>` per row); all three programs print:

```
[t1] alice: first!
[t1] bob: nice post
[t2] alice: following up
```

A few things happened implicitly. In Python and TypeScript the constructor
returned immediately and the scan ran in the background — `query` awaited
readiness for you. In Rust, `.build()` scanned synchronously (there is a
[`build_async()`](../reference/sdk.md#asyncdirsql-rust) for the
non-blocking equivalent). Queries are read-only in every SDK. The complete
contract — constructor parameters, `ready`, value type mappings, error
shapes — is the [SDK reference](../reference/sdk.md).

## 3. React to changes in-process

`watch()` yields the same row events the server streams over
[`/events`](./react-to-changes.md), as your language's native async
iteration:

::: code-group

```python [Python]
async for event in db.watch():
    print(event.action, event.table, event.row)
```

```typescript [TypeScript]
for await (const event of db.watch()) {
  console.log(event.action, event.table, event.row);
}
```

```rust [Rust]
use futures::StreamExt;

let mut stream = db.watch()?;
while let Some(event) = stream.next().await {
    println!("{event:?}");
}
```

:::

Event fields and actions are under
[`RowEvent`](../reference/sdk.md#rowevent).

## Reusing your CLI setup

Everything the config file expresses is available programmatically —
`ignore` patterns, `persist`, extensions — and the constructor's `config`
parameter loads a `.dirsql.toml` directly, so an application can share the
exact table definitions the CLI serves
([constructor reference](../reference/sdk.md#constructor)).
