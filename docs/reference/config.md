# Configuration file (`.dirsql.toml`)

`.dirsql.toml` is a TOML file with one optional `[dirsql]` section, zero or
more `[[dirsql.extension]]` entries, and zero or more `[[table]]` entries.
An empty file is valid. A missing `[dirsql]` section behaves as an
all-defaults one. Unknown keys are a parse error at every level (top level,
`[dirsql]`, `[[table]]`, `[[dirsql.extension]]`) — a typo or a removed key
fails loudly, naming the offending key, rather than silently no-opping.

The [CLI](./cli.md) loads a config only when you pass it with `-c/--config`;
with none given [no named tables](./cli.md#configless-mode) are defined (a
`./.dirsql.toml` on disk is **not** auto-loaded). The [SDKs](./sdk.md) load a
config via the `config` constructor parameter.

**Path resolution.** Relative paths in the config (`[[dirsql.extension]]`
`path`) resolve against the config file's parent
directory. The **index root is not a config concern** — it is decided by the
runner (the CLI's invocation directory, or an SDK's explicit root), never by
the config file's location. See [`--config`](./cli.md#flags).

## `[dirsql]` keys

| Key | Type | Default | Description |
|---|---|---|---|
| `ignore` | array of strings | `[]` | Glob patterns matched against root-relative paths. Matched files are skipped entirely — excluded from the initial scan and from watch events. |
| `hook-timeout` | integer (seconds) | `30` | One global per-run timeout for every `on-file` command hook run. Positive whole seconds; zero and negative values are a config error. See [Command hooks](./hooks.md#timeout). |

The top-level `.dirsql/` directory under the root is always excluded from
scanning, whether or not it appears in `ignore` — it is reserved for
`dirsql`'s own metadata (the persist cache lives there by default). Only the
top-level `.dirsql/` is reserved; a nested `sub/.dirsql/` is an ordinary
directory.

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**"]
hook-timeout = 300
```

Persistence is not a config key. Keep the SQLite index on disk between runs
with the [`--persist [PATH]` CLI flag](./cli.md#dirsql-server) — a machine-local
operational choice that belongs to the runner, not to shareable config.

## `[[dirsql.extension]]`

Each entry declares a SQLite extension to load at startup. Extensions are
loaded onto the connection before any `CREATE TABLE` runs; loading is
enabled only for the duration of each load and disabled again afterwards, so
the SQL `load_extension()` function is never exposed to queries.

| Key | Required | Description |
|---|---|---|
| `path` | yes (non-empty) | The extension's shared library. Either a **file path** (`.so` / `.dylib` / `.dll`; relative paths resolve against the config file's parent directory) or a bare **package name** (no path separator, no loadable-file suffix). The package name is runtime-specific — see [package-name resolution](#package-name-resolution) below. |
| `entrypoint` | no | Init-symbol override. When omitted, SQLite derives the entry point from the filename (`sqlite3_<filename>_init`); set this when that default does not match (e.g. `sqlite-vec`'s entry point is `sqlite3_vec_init`). |

```toml
[[dirsql.extension]]
path       = "./ext/vec0.dylib"
entrypoint = "sqlite3_vec_init"
```

### Package-name resolution

A `path` naming a package is resolved from the
installed package in the runtime environment: the Python launcher and SDK
use `importlib`, the Node launcher and SDK use `require.resolve` against
`node_modules`. Resolution is file-first (a same-named local file wins) and
errors when the package contains zero or multiple loadables for the current
platform. The **standalone Rust binary and the Rust SDK are
file-path-only** — they have no interpreter to resolve package names with.

The name must be what the runtime's resolver knows, which is not always the
name you installed:

- **Python** resolves the **importable module name** — underscores, not the
  pip distribution name. `pip install sqlite-vec` is loaded as
  `path = "sqlite_vec"`; `path = "sqlite-vec"` does not resolve.
- **Node** resolves the package whose install actually **contains the
  loadable**. Meta-packages that split binaries per platform resolve via the
  platform package — e.g. `npm install sqlite-vec` is loaded as
  `path = "sqlite-vec-linux-x64"` (matching your platform), not
  `path = "sqlite-vec"`, whose meta-package ships no loadable.

Extensions add **functions** callable in queries and in a table's DDL. An
extension-backed **virtual table** cannot be declared as a `[[table]]` —
`dirsql` tables are per-file row tables, so a `CREATE VIRTUAL TABLE` DDL is
rejected; call the extension's functions in queries instead.

## `[[table]]`

Each entry maps a glob pattern to a SQL table. A table's columns are exactly
what its required `on-file` command emits — dirsql injects nothing (see
[Columns](./columns.md)).

| Key | Required | Description |
|---|---|---|
| `ddl` | yes | A SQLite `CREATE TABLE` statement. The table name is parsed from it. Only the columns declared here are kept; keys the `on-file` command emits that are not declared are dropped. |
| `glob` | yes | Glob pattern matched against root-relative paths. Every table whose glob matches a file receives that file's rows — a file can populate multiple tables. A `{name}` segment is rewritten to `*` (it matches one path segment but captures nothing). |
| `on-file` | **yes** | A command run once per matched file; its stdout (a JSON array of row objects) becomes the file's rows. Must be non-empty. A `[[table]]` with no `on-file` is a load error (see [parse errors](#parse-errors)). See [Command hooks](./hooks.md#on-file). |
| `strict` | no (default `false`) | When `true`, rows whose keys do not exactly match the declared columns are rejected with an error: extra keys error, and every declared column must be supplied by the `on-file` output. When `false`, extra keys are dropped and missing columns become `NULL`. |

`on-file` is required because a table's rows come from nowhere else. dirsql
does not read file contents or merge filesystem facts on your behalf: the
command reads the file (it receives `{path}`) and prints the rows, and those
rows — filtered to the DDL — are the table. For plain stat columns with no
command, query the path directly with a [path-table](./path-tables.md)
instead of declaring a table.

```toml
[[table]]
ddl     = "CREATE TABLE comments (path TEXT, author TEXT, body TEXT)"
glob    = "_comments/*/*.jsonl"
on-file = "jq -c -s '.' {path}"

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

A row's columns are exactly the keys the command emits, narrowed to the DDL;
dirsql merges nothing else in. Output that is not a JSON array of objects is a
per-file failure: the file is skipped with a stderr warning and the scan
continues (see [failure semantics](./hooks.md#failure-semantics)).

## Composing multiple configs

Pass [`-c`/`--config`](./cli.md#flags) more than once to compose several config
files — a shared team config plus local overrides, or a plugin's config
alongside your own:

```bash
dirsql -c ./.dirsql.toml -c ~/team/embeddings.toml -c ./local.toml
```

The configs load and merge in **argv order**:

- **`[[table]]`, `ignore`, and `[[dirsql.extension]]` entries accumulate** across
  all configs, in order.
- **Each config's `on-file` hooks run from that config file's own
  directory**, under that config's own [`hook-timeout`](#dirsql-keys) — so a
  relative command like `on-file = "sh ./extract.sh"` resolves against the
  config that declared it, wherever it lives.
- Each config is **validated on its own** (the [parse errors](#parse-errors)
  below apply per file). There is no cross-file merge validation, with one
  structural exception: **two configs defining a table of the same name is an
  error**, naming the table.

The index [root](./cli.md#flags) is the invocation directory regardless of where
any config lives. With no `-c`, [no named tables](./cli.md#configless-mode) are
defined (no `./.dirsql.toml` auto-discovery); a single `-c` behaves exactly as
before.

## Parse errors

Loading fails (the CLI enters [degraded mode](./cli.md#degraded-mode); the
SDKs raise/reject) when:

- The TOML is malformed.
- Any table contains an unknown key (top level, `[dirsql]`, `[[table]]`, or
  `[[dirsql.extension]]`). The error names the offending key.
- A `[[table]]` entry omits `ddl` or `glob`.
- A `[[table]]` entry omits `on-file` (or it is empty/whitespace). The error
  names the offending glob and points at the fix:

  > `[[table]] '**/*.md' has no on-file hook, so every row would be all-NULL. Add an `on-file` hook that emits the columns, or, for stat columns with no code, query the path directly: `FROM './'``

- A `[[dirsql.extension]]` entry omits `path`, or `path` is empty.
- `hook-timeout` is zero or negative.

## Full example

```toml
[dirsql]
ignore = ["node_modules/**", ".git/**", "dist/**"]
hook-timeout = 120

[[dirsql.extension]]
path       = "sqlite_vec"            # Python module name; on Node use the
                                     # platform package, e.g. sqlite-vec-linux-x64
entrypoint = "sqlite3_vec_init"

[[table]]
ddl     = "CREATE TABLE comments (author TEXT, body TEXT)"
glob    = "_comments/*/*.jsonl"
on-file = "jq -c -s '.' {path}"

[[table]]
ddl     = "CREATE TABLE documents (title TEXT, summary TEXT)"
glob    = "**/index.md"
on-file = "uv run python extract_doc.py {path}"
```
