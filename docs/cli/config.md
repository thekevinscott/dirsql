---
canonical: https://thekevinscott.github.io/dirsql/cli/config
---

# Configuration File

> Online: <https://thekevinscott.github.io/dirsql/cli/config>

`dirsql` is configured with an optional `.dirsql.toml` file; with none, the
server falls back to [zero-config defaults](./server.md#defaults). A
[TOML](#toml) config is declarative: it defines filesystem-fact tables (the
path, glob captures, and stat metadata) and works with any installation.

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

- **`path`** — the extension's shared library, as either a file path (`.so` /
  `.dylib` / `.dll`; relative paths resolve against the config file's parent
  directory) or a bare **package name**. A package name (no path separator and
  no loadable-file suffix) is resolved from the package installed in the
  runtime env: the Python `dirsql` launcher locates it via `importlib`, the
  Node launcher via `node_modules`, then globs the current platform's loadable
  inside it (a same-named local file wins; zero or multiple matches is an
  error). Package-name resolution needs an interpreter, so it works when the
  config is run through the `dirsql` **CLI** (installed via pip/uv or npm); the
  standalone Rust binary and Rust SDK are file-path-only.
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
applies only to keys produced by an extract callback (relevant for
programmatic [tables](../guide/tables.md)).

### Per-file commands (`on-file`)

Reach for `on-file` when a table's rows come from the *contents* of each
matched file, not just its path and stat metadata. A filesystem-fact table
gives you one row per file; `on-file` runs a command per file that reads the
file and emits as many rows as it likes.

```toml
[[table]]
ddl     = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob    = "**/meta.json"
on-file = "uv run python extract_papers.py {path}"
```

For every file matched by `glob`, `dirsql` runs the command. **The command
reads the file itself and prints a JSON array of row objects on stdout**; each
object becomes one row, its fields mapped to columns:

```json
[
  { "paper_id": "arXiv:2401.001", "title": "On Directories" },
  { "paper_id": "arXiv:2401.002", "title": "SQL All The Way Down" }
]
```

Placeholders substituted into the command:

| Placeholder | Value |
|-------------|-------|
| `{path}`    | The matched file's path **relative to the index root**. Appended automatically when the command omits it, so `extract.py` and `extract.py {path}` behave identically. |
| `{abspath}` | The matched file's absolute path. |
| `{root}`    | The index root directory. |

Filesystem facts (stat virtuals and glob captures) are still merged onto every
`on-file` row, so you can declare `_path`, `_basename`, `{capture}`, etc. in the
DDL alongside the command's own columns — a column emitted by the command wins
over a same-named filesystem fact.

JSON values map to SQLite as follows: `null` → NULL; `true`/`false` → `1`/`0`;
an integer → INTEGER, any other number → REAL; a string → TEXT; a nested array
or object → its JSON text as TEXT.

**Per-file error isolation.** If a file's command fails — a non-zero exit, a
timeout, a spawn error, or output that isn't a JSON array of objects — that
file is skipped (it contributes no rows) and a one-line warning naming the file
and the error is written to stderr. One bad file never aborts the scan; the
other files' rows are indexed normally.

See [Command execution](#command-execution) for the full contract (argv
splitting, injection safety, cwd, environment, timeout, and output framing).

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

## Command execution

Config keys that run an external command — today `on-file`, with more events to
follow — share one execution contract:

- **argv, not a shell.** The command string is split into an argv with
  shell-like quoting (spaces separate arguments; quotes group them), but **no
  shell is invoked** — there is no globbing, piping, `$VAR` expansion, or
  `&&`/`;` chaining. To get those, ask for a shell explicitly:
  `sh -c 'grep foo {path} | sort'` — the quoted script stays a single argument.
- **Injection-safe placeholders.** Each placeholder (`{path}`, `{abspath}`,
  `{root}`, …) is substituted into whole argv tokens, every occurrence, in a
  single left-to-right pass. A substituted value is always exactly one argv
  element, so a path with spaces — or untrusted content that itself contains
  `{…}` or shell metacharacters — is inert and never re-scanned. An unknown
  `{…}` is left literal.
- **Working directory.** The command runs in the **config file's directory**,
  so relative paths in the command resolve predictably regardless of where you
  launched `dirsql`.
- **Environment.** The command inherits `dirsql`'s environment, so tools like
  `uvx --with …` / `npx …` resolve their dependencies as usual.
- **Output framing.** The command's result is the **last non-empty line of
  stdout**; any log/chatter lines above it are ignored. stderr is never data —
  it is captured only to enrich error messages.
- **Timeout.** Each command run is bounded by a fixed **30-second** timeout (no
  per-table override yet); a command that exceeds it is killed and treated as a
  failure.
- **Errors.** A non-zero exit, a timeout, a spawn failure, or output that does
  not parse as expected is a per-file failure: the file is skipped with a
  stderr warning and the scan continues.
