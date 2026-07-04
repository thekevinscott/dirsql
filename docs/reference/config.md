# Configuration file (`.dirsql.toml`)

`.dirsql.toml` is a TOML file with one optional `[dirsql]` section, zero or
more `[[dirsql.extension]]` entries, and zero or more `[[table]]` entries.
An empty file is valid. A missing `[dirsql]` section behaves as an
all-defaults one. Unknown keys are ignored.

The [CLI](./cli.md) loads `./.dirsql.toml` by default (`--config <path>`
overrides). The [SDKs](./sdk.md) load a config via the `config` constructor
parameter.

**Path resolution.** Relative paths in the config (`root`, `persist_path`,
`[[dirsql.extension]]` `path`) resolve against the config file's parent
directory. The scan root defaults to that same directory when `root` is not
set.

## `[dirsql]` keys

| Key | Type | Default | Description |
|---|---|---|---|
| `root` | string | config file's parent directory | Directory to index. Relative values resolve against the config file's parent. An explicit `root` passed to an SDK constructor overrides this (a warning is emitted on stderr). |
| `ignore` | array of strings | `[]` | Glob patterns matched against root-relative paths. Matched files are skipped entirely — excluded from the initial scan and from watch events. |
| `persist` | boolean | `false` | Keep the SQLite index on disk between runs. When `false`, the database is rebuilt in memory on every startup. |
| `persist_path` | string | `<root>/.dirsql/cache.db` | Location of the on-disk cache. Relative values resolve against the config file's parent. Ignored unless `persist = true`. |
| `pre-query` | string | none | Server-wide command hook: the raw `POST /query` request body is passed to this command as `{args}`, and the plain-text SQL it prints is executed instead of parsing the body as `{"sql": …}`. CLI server only; the SDKs ignore it. Must be non-empty. See [Command hooks](./hooks.md#pre-query). |
| `post-query` | string | none | Server-wide command hook: each successful `POST /query` result set is handed to this command (as a JSON array on stdin, and as `{args}` up to 96 KiB), and the JSON body it prints is returned instead of the bare row array. CLI server only; the SDKs ignore it. Must be non-empty. See [Command hooks](./hooks.md#post-query). |
| `hook-timeout` | integer (seconds) | `30` | One global per-run timeout for every command hook — `on-file`, `pre-query`, and `post-query` alike. Positive whole seconds; zero and negative values are a config error. See [Command hooks](./hooks.md#timeout). |

The top-level `.dirsql/` directory under the root is always excluded from
scanning, whether or not it appears in `ignore` — it is reserved for
`dirsql`'s own metadata (the persist cache lives there by default). Only the
top-level `.dirsql/` is reserved; a nested `sub/.dirsql/` is an ordinary
directory.

```toml
[dirsql]
root = "../data"
ignore = ["node_modules/**", ".git/**"]
persist = true
persist_path = ".dirsql/cache.db"   # the default; shown for illustration
hook-timeout = 300
```

## `[[dirsql.extension]]`

Each entry declares a SQLite extension to load at startup. Extensions are
loaded onto the connection before any `CREATE TABLE` runs; loading is
enabled only for the duration of each load and disabled again afterwards, so
the SQL `load_extension()` function is never exposed to queries.

| Key | Required | Description |
|---|---|---|
| `path` | yes (non-empty) | The extension's shared library. Either a **file path** (`.so` / `.dylib` / `.dll`; relative paths resolve against the config file's parent directory) or a bare **package name** (no path separator, no loadable-file suffix). |
| `entrypoint` | no | Init-symbol override. When omitted, SQLite derives the entry point from the filename (`sqlite3_<filename>_init`); set this when that default does not match (e.g. `sqlite-vec`'s entry point is `sqlite3_vec_init`). |

```toml
[[dirsql.extension]]
path       = "./ext/vec0.dylib"
entrypoint = "sqlite3_vec_init"
```

**Package-name resolution.** A `path` naming a package is resolved from the
installed package in the runtime environment: the Python launcher and SDK
use `importlib`, the Node launcher and SDK use `require.resolve` against
`node_modules`. Resolution is file-first (a same-named local file wins) and
errors when the package contains zero or multiple loadables for the current
platform. The **standalone Rust binary and the Rust SDK are
file-path-only** — they have no interpreter to resolve package names with.

Extensions add **functions** callable in queries and in a table's DDL. An
extension-backed **virtual table** cannot be declared as a `[[table]]` —
`dirsql` tables are per-file row tables, so a `CREATE VIRTUAL TABLE` DDL is
rejected; call the extension's functions in queries instead.

## `[[table]]`

Each entry maps a glob pattern to a SQL table. Every matched file produces
rows whose columns come from filesystem facts — [glob captures and virtual
columns](./columns.md) — plus, when `on-file` is set, the output of a
per-file command.

| Key | Required | Description |
|---|---|---|
| `ddl` | yes | A SQLite `CREATE TABLE` statement. The table name is parsed from it. Only columns declared here are populated; auto-injected facts not in the DDL are dropped. |
| `glob` | yes | Glob pattern matched against root-relative paths. May contain `{name}` [capture segments](./columns.md#glob-captures). First matching table wins when a file matches several globs. |
| `strict` | no (default `false`) | When `true`, rows whose keys do not exactly match the declared columns are rejected with an error: extra keys error, and every declared column must be supplied (by the command/extract output, a glob capture, or a virtual column). When `false`, extra keys are dropped and missing columns become `NULL`. |
| `on-file` | no | A command run once per matched file; its stdout (a JSON array of row objects) becomes the file's rows. Must be non-empty. See [Command hooks](./hooks.md#on-file). |

Without `on-file`, a table produces exactly one row per matched file, built
entirely from filesystem facts. Content interpretation (frontmatter, JSON
fields, CSV parsing) is out of scope for plain config tables — use
`on-file`, or a programmatic [SDK table](./sdk.md#table) with an `extract`
callback.

```toml
[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"

[[table]]
ddl     = "CREATE TABLE papers (paper_id TEXT, title TEXT)"
glob    = "**/meta.json"
on-file = "uv run python extract_papers.py {path}"
strict  = true
```

### `on-file` row mapping

The command prints a JSON array of objects; each object becomes one row.
JSON values map to SQLite as: `null` → `NULL`; `true`/`false` → `1`/`0`; an
integral number → `INTEGER`, any other number → `REAL`; a string → `TEXT`; a
nested array or object → its JSON text as `TEXT`.

Filesystem facts are still merged onto every `on-file` row; a column emitted
by the command wins over a same-named fact. Output that is not a JSON array
of objects is a per-file failure: the file is skipped with a stderr warning
and the scan continues (see [failure semantics](./hooks.md#failure-semantics)).

## Parse errors

Loading fails (the CLI enters [degraded mode](./cli.md#degraded-mode); the
SDKs raise/reject) when:

- The TOML is malformed.
- A `[[table]]` entry omits `ddl` or `glob`.
- A `[[dirsql.extension]]` entry omits `path`, or `path` is empty.
- `on-file`, `pre-query`, or `post-query` is present but empty/whitespace.
- `hook-timeout` is zero or negative.

## Full example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]
persist = true
pre-query = "uv run python to_sql.py {args}"
post-query = "jq -c '{results: .}'"
hook-timeout = 120

[[dirsql.extension]]
path       = "sqlite-vec"            # package name (pip/npm installs)
entrypoint = "sqlite3_vec_init"

[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"

[[table]]
ddl  = "CREATE TABLE documents (_path TEXT, _basename TEXT, _size INTEGER)"
glob = "**/index.md"
```
