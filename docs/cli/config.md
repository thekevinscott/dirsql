---
canonical: https://thekevinscott.github.io/dirsql/cli/config
---

# Configuration File

> Online: <https://thekevinscott.github.io/dirsql/cli/config>

`dirsql` can be configured with an optional `.dirsql.toml` file (if omitted, server falls back to [defaults](./server.md#defaults)). `.dirsql.toml` defines how files are parsed into SQL tables.

## Basic Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
name = "posts"
glob = "posts/*.md"

  [[table.column]]
  name = "_path"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_size"
  type = "INTEGER"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"
```

Each table is defined by a `name`, a `glob`, and a list of `[[table.column]]`
blocks (each with a `name` and a `type`). Every `posts/*.md` file produces one
row in the `posts` table.

::: tip Deprecated: `ddl`
Earlier versions defined a table with a single `ddl = "CREATE TABLE ..."` key
instead of `name` + `[[table.column]]` blocks. The `ddl` key still works but is
**deprecated** and slated for removal; use the structured form for new configs.
:::

## Loading a Config File

Pass the config file path to the `DirSQL` constructor:

::: code-group

```python [Python]
from dirsql import DirSQL

db = DirSQL(config="./my-project/.dirsql.toml")
await db.ready()
```

```rust [Rust]
use dirsql::DirSQL;

let db = DirSQL::builder()
    .config("./my-project/.dirsql.toml")
    .build()?;
```

```typescript [TypeScript]
import { DirSQL } from "dirsql";

// String argument is interpreted as a config file path.
const db = new DirSQL("./my-project/.dirsql.toml");
await db.ready;
```

:::

By default, the root directory scanned is the config file's parent
directory. Override it by passing `root` explicitly (the explicit value
wins and a warning is emitted) or by declaring `[dirsql].root` in the
config file itself.

## Root Directory

By default, the config file's parent directory is the scan root. To index
a different location, declare `[dirsql].root` (relative paths are resolved
relative to the config file's parent):

```toml
[dirsql]
root = "../data"
ignore = ["node_modules/**"]
```

## Stat Virtuals

Every config-defined table can expose any of these reserved columns. Add
the ones you want as `[[table.column]]` entries; the rest are silently dropped.

| Column | Type    | Source |
|--------|---------|--------|
| `_path`     | TEXT    | The file's path relative to the scan root. |
| `_basename` | TEXT    | The filename including extension. |
| `_dir`      | TEXT    | The parent directory path (relative to root). |
| `_ext`      | TEXT    | The file extension, lowercased, no leading dot. |
| `_size`     | INTEGER | Size in bytes. |
| `_mtime`    | INTEGER | Last-modified time, unix seconds. |
| `_ctime`    | INTEGER | Created/changed time, unix seconds. |

Example query:

```sql
SELECT _basename, _size
FROM posts
WHERE _mtime > strftime('%s', '2024-01-01')
ORDER BY _mtime DESC;
```

## Path Captures

Use `{name}` in glob patterns to extract path segments as columns. Add a
matching `[[table.column]]` entry and the capture is auto-populated:

```toml
[[table]]
name = "comments"
glob = "_comments/{thread_id}/*.jsonl"

  [[table.column]]
  name = "thread_id"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"
```

A file at `_comments/abc123/2024-05-05.jsonl` produces a row with
`thread_id = "abc123"`, `_basename = "2024-05-05.jsonl"`, and `_mtime` set
to the file's modification time.

## Ignore Patterns

The `ignore` list skips files and directories entirely (not even scanned):

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "*.pyc", "__pycache__/**"]
```

The top-level `.dirsql/` directory is always excluded, whether you list it
or not — it is a reserved namespace for `dirsql`'s own metadata (see
[Persistence](../guide/persistence.md)).

## Persistence

Set `persist = true` to keep the SQLite database on disk between runs
instead of rebuilding from scratch on every startup:

```toml
[dirsql]
persist = true
# persist_path = ".dirsql/cache.db"   # optional; this is the default
```

See [Persistence](../guide/persistence.md) for the full reconcile algorithm,
storage layout, and limitations.

## Strict Mode

By default, auto-injected virtuals that aren't declared as columns are
silently dropped, and undeclared user-extract keys are dropped. Enable strict
mode to error when an extract emits keys not declared in the table's columns:

```toml
[[table]]
name = "comments"
glob = "_comments/{thread_id}/*.jsonl"
strict = true

  [[table.column]]
  name = "thread_id"
  type = "TEXT"
```

Strict mode does **not** apply to auto-injected stat virtuals — those are
always filtered to the declared columns regardless. Strict mode
applies only to keys produced by an extract callback (relevant for
programmatic [tables](../guide/tables.md)).

## Full Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]

[[table]]
name = "comments"
glob = "_comments/{thread_id}/*.jsonl"

  [[table.column]]
  name = "thread_id"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"

[[table]]
name = "documents"
glob = "**/index.md"

  [[table.column]]
  name = "_path"
  type = "TEXT"

  [[table.column]]
  name = "_basename"
  type = "TEXT"

  [[table.column]]
  name = "_size"
  type = "INTEGER"

[[table]]
name = "logs"
glob = "logs/*.csv"

  [[table.column]]
  name = "_path"
  type = "TEXT"

  [[table.column]]
  name = "_size"
  type = "INTEGER"

  [[table.column]]
  name = "_mtime"
  type = "INTEGER"
```

## When you need parsed content

`.dirsql.toml` does not parse file contents. For columns derived from the
*inside* of files (frontmatter keys, JSON values, CSV cells, etc.),
register a programmatic [`Table`](../guide/tables.md) instead, and parse the
bytes in your host language. Glob captures and stat virtuals are still
auto-injected into rows produced by your extract.
