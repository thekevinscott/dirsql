---
canonical: https://thekevinscott.github.io/dirsql/guide/config
---

# Configuration File

> Online: <https://thekevinscott.github.io/dirsql/guide/config>

`dirsql` can be configured with a `.dirsql.toml` file. Tables defined this
way produce **one row per matched file**. Each row's columns come from
filesystem facts:

- **Glob path captures** — named `{placeholder}` segments in the glob.
- **Stat virtuals** — reserved `_`-prefixed columns for path-derived and
  stat-derived metadata.

Content interpretation (parsing JSON, CSV, frontmatter, etc.) is **not**
configured in `.dirsql.toml`. If you need columns derived from file
contents, register a programmatic [`Table`](./tables.md) whose `extract`
function does the parsing in your host language.

## Basic Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl  = "CREATE TABLE posts (_path TEXT, _basename TEXT, _size INTEGER, _mtime INTEGER)"
glob = "posts/*.md"
```

Each `posts/*.md` file produces one row. The DDL declares which stat
virtuals are surfaced as SQL columns.

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
the ones you want to your DDL; the rest are silently dropped.

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
matching column name to the DDL and the capture is auto-populated:

```toml
[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"
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
[Persistence](./persistence.md)).

## Persistence

Set `persist = true` to keep the SQLite database on disk between runs
instead of rebuilding from scratch on every startup:

```toml
[dirsql]
persist = true
# persist_path = ".dirsql/cache.db"   # optional; this is the default
```

See [Persistence](./persistence.md) for the full reconcile algorithm,
storage layout, and limitations.

## Strict Mode

By default, auto-injected virtuals that aren't in the DDL are silently
dropped, and undeclared user-extract keys are dropped. Enable strict mode
to error when an extract emits keys not declared in the DDL:

```toml
[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT)"
glob = "_comments/{thread_id}/*.jsonl"
strict = true
```

Strict mode does **not** apply to auto-injected stat virtuals — those are
always filtered to the DDL's declared columns regardless. Strict mode
applies only to keys produced by an extract callback (relevant for
programmatic [tables](./tables.md)).

## Full Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]

[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"

[[table]]
ddl  = "CREATE TABLE documents (_path TEXT, _basename TEXT, _size INTEGER)"
glob = "**/index.md"

[[table]]
ddl  = "CREATE TABLE logs (_path TEXT, _size INTEGER, _mtime INTEGER)"
glob = "logs/*.csv"
```

## When you need parsed content

`.dirsql.toml` does not parse file contents. For columns derived from the
*inside* of files (frontmatter keys, JSON values, CSV cells, etc.),
register a programmatic [`Table`](./tables.md) instead, and parse the
bytes in your host language. Glob captures and stat virtuals are still
auto-injected into rows produced by your extract.

For tabular formats specifically, [DuckDB](https://duckdb.org) is also
worth considering: it has built-in `read_csv`, `read_json`, and
`read_parquet` over file globs, plus a community
[markdown extension](https://github.com/teaguesterling/duckdb_markdown).
