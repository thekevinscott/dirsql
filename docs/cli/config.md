---
canonical: https://thekevinscott.github.io/dirsql/cli/config
---

# Configuration File

> Online: <https://thekevinscott.github.io/dirsql/cli/config>

`dirsql` is configured with an optional config file; with none, the server
falls back to [zero-config defaults](./server.md#defaults). Choose a format by
what you need:

- **[TOML](#toml)** — declarative; defines filesystem-fact tables (the path,
  glob captures, and stat metadata). Works with any installation.
- **[Python](#python)** and **[JavaScript](#javascript)** — native-language
  configs that build tables from the *contents* of files (frontmatter, JSON
  values, CSV cells) through a dynamic `extract` callback. CLI-only; only the
  launcher matching the file's language can run it.

## TOML

Reach for a TOML config — the default `.dirsql.toml` — to declare tables from
filesystem facts: a glob selects files, and columns come from path captures and
stat metadata. No code required, and it works with every installation.

### Basic Example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]

[[table]]
ddl  = "CREATE TABLE posts (_path TEXT, _basename TEXT, _size INTEGER, _mtime INTEGER)"
glob = "posts/*.md"
```

Each `posts/*.md` file produces one row in the `posts` table.

### Loading a Config File

The CLI loads `./.dirsql.toml` by default; pass `--config <path>` to point at
another file. To load the same `.toml` from the SDK, pass its path to the
`DirSQL` constructor:

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

### Root Directory

By default, the config file's parent directory is the scan root. To index
a different location, declare `[dirsql].root` (relative paths are resolved
relative to the config file's parent):

```toml
[dirsql]
root = "../data"
ignore = ["node_modules/**"]
```

### Stat Virtuals

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

### Path Captures

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

### Ignore Patterns

The `ignore` list skips files and directories entirely (not even scanned):

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "*.pyc", "__pycache__/**"]
```

The top-level `.dirsql/` directory is always excluded, whether you list it
or not — it is a reserved namespace for `dirsql`'s own metadata (see
[Persistence](../guide/persistence.md)).

### Persistence

Set `persist = true` to keep the SQLite database on disk between runs
instead of rebuilding from scratch on every startup:

```toml
[dirsql]
persist = true
# persist_path = ".dirsql/cache.db"   # optional; this is the default
```

See [Persistence](../guide/persistence.md) for the full reconcile algorithm,
storage layout, and limitations.

### Loading extensions

You can load SQLite extensions by specifying them in a config.

Declare each extension as a `[[dirsql.extension]]` entry:

```toml
[[dirsql.extension]]
path       = "./ext/myext.dylib"
entrypoint = "sqlite3_myext_init"
```

- **`path`** — a path to the extension's shared library (`.so` / `.dylib` /
  `.dll`). Relative paths resolve against the config file's parent directory.
- **`entrypoint`** *(optional)* — the extension's init symbol. When omitted,
  SQLite derives a default from the filename; set it when that default does not
  match (for example, `sqlite-vec`'s entry point is `sqlite3_vec_init`).

**Note**: `dirsql` enables extension loading only while loading the configured libraries,
then disables it again, so `load_extension()` is not exposed via SQL to the user.

Extensions add **functions** you can call in queries and in a regular table's
DDL (defaults, generated columns). An extension-backed **virtual table** cannot
be declared as a `[[table]]` — `dirsql` tables are per-file row tables — so a
`CREATE VIRTUAL TABLE` DDL is rejected; call the extension's functions in your
queries instead.

### Strict Mode

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
applies only to keys produced by an extract callback (relevant for the
[Python](#python) / [JavaScript](#javascript) configs below and programmatic
[tables](../guide/tables.md)).

### Full Example

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

## Python

Reach for a Python config when your columns come from the *contents* of a
file — parsed JSON, frontmatter, CSV cells — rather than from filesystem
facts alone. You write a dynamic `extract` callback in Python, and the file
otherwise looks exactly like the in-process SDK construction (same `DirSQL` /
`Table` API):

```bash
dirsql --config dirsql.config.py
```

```python [dirsql.config.py]
import json
from dirsql import DirSQL, Table

def extract_meta(path):
    with open(path) as f:
        return [json.load(f)]

# Python must export a module-level `app`.
app = DirSQL(
    root="papers",  # required — see "Set a root" below
    tables=[
        Table(
            ddl="CREATE TABLE papers (title TEXT, _path TEXT)",
            glob="**/meta.json",
            extract=extract_meta,
        ),
    ],
)
```

`extract` receives the path of each matched file and returns a list of rows
(one dict per row).

## JavaScript

A JavaScript config gives you the same contents-driven `extract` in Node,
in either ES module or CommonJS form:

```bash
dirsql --config dirsql.config.mjs
```

::: code-group

```javascript [dirsql.config.mjs]
import { readFileSync } from "node:fs";
import { DirSQL } from "dirsql";

export default new DirSQL({
  root: "papers", // required — see "Set a root" below
  tables: [
    {
      ddl: "CREATE TABLE papers (title TEXT, _path TEXT)",
      glob: "**/meta.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});
```

```javascript [dirsql.config.cjs]
const { readFileSync } = require("node:fs");
const { DirSQL } = require("dirsql");

module.exports = new DirSQL({
  root: "papers", // required — see "Set a root" below
  tables: [
    {
      ddl: "CREATE TABLE papers (title TEXT, _path TEXT)",
      glob: "**/meta.json",
      extract: (path) => [JSON.parse(readFileSync(path, "utf8"))],
    },
  ],
});
```

:::

## Notes for native-language configs

These apply to both the Python and JavaScript forms above.

- **Export the config.** Python exposes a module-level `app = DirSQL(...)`; an
  ES module (`.mjs`, or `.js` in an ESM package) uses
  `export default new DirSQL(...)`; CommonJS (`.cjs`, or `.js` in a CJS
  package) uses `module.exports = new DirSQL(...)`. Only the extension
  matters — the file can be named anything; `dirsql.config.{py,mjs,cjs}` is the
  suggested convention, not a requirement.
- **Set a `root`.** Unlike TOML configs (which default the scan root to the
  config file's directory), native-language configs require an explicit `root`.
  Without one the Python launcher errors and the JavaScript launcher silently
  indexes nothing.
- **Install the launcher on your `PATH`.** To run your `extract`, the server
  spawns `dirsql interpret`, so the matching `dirsql` launcher must be installed
  and on your `PATH` — a global `pip`/`uv` install for `.py`, or `npm` for
  `.mjs` / `.cjs`. Only the launcher matching the file's language can run it.

## Why no Rust config?

`dirsql` accepts native-language configs in Python and JavaScript because the
CLI runs your `extract` by spawning an interpreter (`dirsql interpret`). Rust is
compiled ahead of time, so there is no interpreter to spawn — a Rust config
would mean compiling your code at server startup. Rust users have two better
options:

- **Embed the [Rust SDK](https://github.com/thekevinscott/dirsql/tree/main/packages/rust)
  directly** — `DirSQL::builder()` with an `extract` written in Rust, no config
  file needed.
- **Load a compiled SQLite extension** via
  [`[[dirsql.extension]]`](#loading-extensions) — a `.so` / `.dylib` / `.dll`
  you can write in Rust — to add functions you call from your queries.
