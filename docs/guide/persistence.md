# Persistence

By default `dirsql` keeps its SQLite database in memory and rebuilds it from scratch every time the process starts. For large directories this can take seconds to minutes -- nearly all of which is spent re-parsing files that haven't changed since the previous run.

Persistence stores the SQLite database on disk so that subsequent startups only re-parse the files that have actually changed.

::: tip Same answers, faster startup
The rows returned by `query()` after a persistent startup are equivalent to those produced by a from-scratch rebuild. Persistence is a startup-time optimization, not a correctness compromise. The reconcile algorithm is the same one `git status` uses to decide which files have changed since the last index write.
:::

## Quick start

::: code-group

```toml [.dirsql.toml]
[dirsql]
persist = true
```

```python [Python]
from dirsql import DirSQL

db = DirSQL("./my-project", tables=[...], persist=True)
await db.ready()
```

```rust [Rust]
use dirsql::DirSQL;

let db = DirSQL::builder()
    .root("./my-project")
    .tables(vec![/* ... */])
    .persist(true)
    .build()?;
```

```typescript [TypeScript]
import { DirSQL } from "dirsql";

const db = new DirSQL({ root: "./my-project", tables: [/* ... */], persist: true });
await db.ready;
```

:::

That's it. The first run writes the database to `./my-project/.dirsql/cache.db`. Every subsequent startup uses the cache.

## Configuration

| Option | Type | Default | Meaning |
|---|---|---|---|
| `persist` | boolean | `false` | Enable persistent on-disk storage. |
| `persist_path` (Python, Rust) / `persistPath` (TypeScript) | string | `<root>/.dirsql/cache.db` | Override the database file path. Ignored when `persist` is `false`. |

The default location keeps the cache alongside the data it indexes, which means it follows the project around (clone, copy, move) without extra setup. Override `persist_path` if you want the cache somewhere else -- a CI cache directory, a tmpfs mount, an XDG cache dir, etc.

::: code-group

```toml [.dirsql.toml]
[dirsql]
persist = true
persist_path = "/var/cache/dirsql/myproject.db"
```

```python [Python]
db = DirSQL(
    "./my-project",
    tables=[...],
    persist=True,
    persist_path="/var/cache/dirsql/myproject.db",
)
```

```rust [Rust]
let db = DirSQL::builder()
    .root("./my-project")
    .tables(vec![/* ... */])
    .persist(true)
    .persist_path("/var/cache/dirsql/myproject.db")
    .build()?;
```

```typescript [TypeScript]
const db = new DirSQL({
  root: "./my-project",
  tables: [/* ... */],
  persist: true,
  persistPath: "/var/cache/dirsql/myproject.db",
});
```

:::

## The `.dirsql/` directory

`dirsql` reserves the top-level `.dirsql/` directory inside every scanned root. It is **unconditionally excluded from the directory walk**, whether persistence is enabled or not. This means:

- The default cache path `<root>/.dirsql/cache.db` cannot accidentally be ingested as a data file.
- You can place additional `dirsql`-related files in `.dirsql/` (e.g. a project-local config snapshot) without them being parsed.
- You should not put your own data files in `.dirsql/` -- they will be silently ignored.

If you persist into `.dirsql/`, add it to your `.gitignore`:

```
.dirsql/
```

The cache file should never be committed -- it is reproducible from the source tree and frequently large.

## How the startup reconcile works

When a persistent cache exists, `dirsql` does not blindly trust it. On startup it:

1. **Checks compatibility metadata.** If the cached `dirsql` version, schema version, glob configuration, parser versions, or canonical root path differs from the current build, the cache is wiped and rebuilt from scratch.
2. **Walks the tree and stats every matching file.** This is metadata-only -- no file contents are read.
3. **For each file, compares the live `(size, mtime, ctime, inode, dev)` tuple against the cached row:**
   - **Trust the cache** when every field matches *and* the file's mtime is older than the cache's snapshot time (outside the racy window).
   - **Hash-confirm** when the tuple matches but the file's mtime falls inside the racy window. `dirsql` reads and hashes the file; if the hash matches the cached hash, the cache is trusted.
   - **Re-parse** when any field of the tuple differs.
4. **Deletes** rows for files that were in the cache but are no longer on disk.
5. **Inserts** rows for files that are on disk but were not in the cache.

This is the same algorithm `git status` uses to decide which files have changed since the last index write. The "racy window" handling is what closes the gap when a file is modified within the same filesystem-timestamp resolution as the cache write.

## When `dirsql` does a full rebuild

Any of the following will cause the cache to be discarded and rebuilt from scratch on the next startup:

- The `dirsql` library was upgraded between runs.
- The glob configuration changed (a new table, a removed table, a modified glob, a changed `ignore` list).
- A built-in parser version changed (this generally only happens on `dirsql` upgrades).
- The cache was written for a different root directory than the one currently configured.
- The internal schema of the cache changed (i.e. you upgraded `dirsql` across a schema version bump).

Full rebuilds take exactly as long as a non-persistent startup -- there is no penalty for them, only a missed optimization.

## Limitations

### Network filesystems

NFS, SMB/CIFS, and similar network filesystems cache file attributes on the client and can return stale `stat` results. Persistent mode is **not supported** on network filesystems and may produce stale rows. Use in-memory mode (the default) if your `root` lives on a network mount.

### The mtime-preservation edge case

Racy-stat detection misses changes only when **all** of the following are true:

- A file's contents are modified.
- The file's size after modification is identical to its size before.
- The file's `mtime` is externally reset to a value older than the cache's snapshot time (e.g. via `touch -r` or a backup-restore tool that preserves mtime).

If you cannot tolerate this edge case, disable persistence (`persist = false`). This is the same trade-off `git` makes with `core.trustctime` / `core.checkStat`.

### Single writer

Only one `dirsql` process should write to a given cache file at a time. Multiple read-only processes can query the same file safely once the writer finishes the initial reconcile. Coordinated multi-writer access is not supported in v0.3.0.

## Inspecting the cache

The persistent database is a normal SQLite file. You can open it with any SQLite client:

```bash
sqlite3 .dirsql/cache.db
```

```sql
.tables
-- comments  documents  metrics  _dirsql_files  _dirsql_meta

SELECT * FROM _dirsql_meta;
-- schema_version    | 1
-- dirsql_version    | 0.3.0
-- glob_config_hash  | <hex>
-- parser_versions   | {"json":"1","jsonl":"1","csv":"1",...}
-- root_canonical    | /home/alice/my-project
```

The `_dirsql_files` and `_dirsql_meta` tables are managed by `dirsql`. Do not modify them by hand -- on the next startup, `dirsql` will detect the inconsistency and rebuild from scratch.
