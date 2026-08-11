# CLI

Query is the default: `dirsql "<sql>"` runs one query and prints JSON rows.
The `dirsql` binary has these modes:

| Invocation | Behavior |
|---|---|
| `dirsql "<sql>"` | Run one query over the directory and print the rows as JSON. The default; identical to `dirsql query "<sql>"`. |
| `dirsql query "<sql>"` | Explicit synonym for the default one-shot query. |
| `dirsql server` | Start a long-lived HTTP server exposing a SQL view of a directory. See [HTTP API](./http-api.md). |
| `dirsql init` | Generate a `.dirsql.toml`. |

Bare `dirsql` with no SQL is a usage error pointing at `dirsql server` — it
does **not** start the server.

## Installation

::: code-group

```bash [npm]
npx dirsql "SELECT * FROM './'"
```

```bash [PyPI]
uvx dirsql "SELECT * FROM './'"
```

```bash [Cargo]
# The `cli` feature is opt-in; this installs the binary only.
cargo install dirsql --features cli
dirsql "SELECT * FROM './'"
```

:::

The npm launcher requires **Node ≥ 20.11**.

## Default query mode

```bash
# No -c: query the filesystem with a path-table.
dirsql "SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5"
# [{"basename":"model.bin","size":104857600}, …]
```

`dirsql "<sql>"` is exactly [`dirsql query "<sql>"`](#dirsql-query) — same
pipeline, same flags, same output. See that section for config discovery,
`--persist`, `--on-file`, hooks, and exit codes.

## `dirsql server`

```bash
dirsql server
# Running at localhost:7117
```

On startup the server prints `Running at <host>:<port>` to stdout. It runs
until it receives `SIGINT` (Ctrl-C) or `SIGTERM`, then drains in-flight
requests, closes open `/events` streams, and exits.

### Flags

Config flags are subcommand-local: pass them after `server`
(`dirsql server -c <cfg>`).

| Flag | Default | Description |
|---|---|---|
| `-c, --config <path>` | none | Path to a [config file](./config.md). **Repeatable** (`-c a -c b`): the configs load and merge in argv order — see [Composing multiple configs](./config.md#composing-multiple-configs). The index is always rooted at the **invocation directory** (the current working directory), regardless of where a config lives — so `--config /elsewhere/.dirsql.toml` still indexes the directory you ran `dirsql server` from. With none given, **no named tables are defined** — query the filesystem with a [path-table](./path-tables.md) (`FROM './'`). A `./.dirsql.toml` on disk is **not** auto-loaded; pass it explicitly. A `-c` naming a file that does not exist is an [error](#degraded-mode). |
| `--host <addr>` | `localhost` | Bind address. |
| `--port <n>` | `7117` | TCP port to bind. |
| `--persist [<path>]` | off | Keep the SQLite index on disk between runs so a restart only re-parses files that actually changed. Bare `--persist` caches at `<root>/.dirsql/cache.db`; `--persist <path>` caches at `<path>`. Off by default (the index is ephemeral). Also available on [`dirsql query`](#dirsql-query). See [Keep the index across restarts](../howto/persist.md). |
| `--no-ignore` | off | Scan files a `.gitignore` would hide. [Path-tables](./path-tables.md#skip-rules) respect `.gitignore` files by default; this flag restores the full walk. The built-in skips (`node_modules`/`.git`) and configured `ignore` patterns still apply. Also available on [`dirsql query`](#dirsql-query). |
| `--extension <path>` | none | Load a SQLite extension by literal path, overriding the config's `[[dirsql.extension]]` entries. Repeatable. Format: `<path>` or `<path>::<entrypoint>`. Internal plumbing for the pip/npm launchers, which resolve package-name extensions and pass the resolved paths here — not intended for direct use. When any `--extension` is present, the config file's own extension entries are not loaded. |
| `--version` | | Print the version and exit. |
| `--help` | | Print usage and exit. |

### Defaults

- Per-query timeout: **30 seconds**, in server mode only. A query exceeding
  it returns `408 Request Timeout`. One-shot [`dirsql query`](#dirsql-query)
  has no built-in timeout.
- `on-file` command hooks run **unbounded**; bound one by wrapping its
  command in `timeout(1)` (see
  [Bounding a hook](./hooks.md#bounding-a-hook)).

### Configless mode

With no `-c/--config`, the server indexes the invocation directory but
defines **no named tables**. Filesystem queries go through
[path-tables](./path-tables.md): a quoted path in place of a table name,
scanned live. A `./.dirsql.toml` sitting in the current directory is **not**
auto-loaded (pass it with `-c ./.dirsql.toml` to use it).

`'./'` is the whole root — every file at any depth, one row per file, with
all seven [stat columns](./columns.md): `path`, `basename`, `dir`, `ext`,
`size`, `mtime`, `ctime`.

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT basename, size FROM \'./\' ORDER BY size DESC LIMIT 5"}'
```

Earlier versions served an implicit table named `files` here. It is gone; a
`SELECT ... FROM files` with no config now fails and points at `FROM './'`.

Passing a config with `-c` fully overrules this default. A `-c` naming a file
that does not exist is an error (not a fallback to the default); a config that
exists but fails to load degrades the server (see below).

### Degraded mode

When a config passed with `-c` cannot be resolved or loaded — the file does
not exist, is unreadable, or has invalid TOML / schema errors — the server
still starts and binds, but every request to `/query` and `/events` returns
`503 Service Unavailable` with a JSON body describing the failure (the
diagnostic names the offending path or key):

```json
{"error": "failed to load config: ..."}
```

The one-shot [`dirsql query`](#dirsql-query) surfaces the same failure as a
non-zero exit with the diagnostic on stderr.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean shutdown after `SIGINT` / `SIGTERM`. |
| `1` | Failed to bind `host:port`, or an error during shutdown. |

## `dirsql query`

Run a SQL query from the shell:

```bash
# No -c: query the filesystem with a path-table.
dirsql query "SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5"
# [{"basename":"model.bin","size":104857600}, …]

# A config table (`posts`) needs its config passed explicitly, AFTER the subcommand.
dirsql query "SELECT COUNT(*) AS n FROM posts" -c ./.dirsql.toml | jq '.[0].n'
```

::: warning Config flags are subcommand-local
Pass `-c`/`--config`, `--persist`, and `--extension` **after** `query`
(`dirsql query "<sql>" -c <cfg>`). A config flag placed *before* the subcommand
is a hard error — `error: the subcommand 'query' cannot be used with
'--config <CONFIG>'` — never silently dropped. (The default mode without the
`query` keyword takes the same flags after the SQL: `dirsql "<sql>" -c <cfg>`;
for the server they follow the subcommand: `dirsql server -c <cfg>`.)
:::

The subcommand builds the index, runs the SQL, prints the result rows as a
JSON array on stdout (byte-identical to the [`POST /query`](./http-api.md)
response body), and exits `0`.

`dirsql query` is a thin adapter over the **same query pipeline the server
uses**, so behavior is identical to `POST /query` by construction:

- **Config discovery** honors `--config` passed after the subcommand (with none
  given, [no named tables](#configless-mode)), and `--extension` overrides,
  exactly as server mode does.
- **`--persist [<path>]`** is honored, so a repeated `dirsql query` reuses the
  on-disk cache. Because its value is optional, place a bare `--persist` after
  the SQL (`dirsql query "SELECT …" --persist`) or use the `=` form
  (`--persist=/path`) so it does not swallow the SQL argument.
- **`--no-ignore`** is honored: path-tables in the query scan files a
  `.gitignore` would hide. See [Skip rules](./path-tables.md#skip-rules).
- **`on-file` hooks** apply identically (unbounded; wrap in `timeout(1)` to
  bound — see [Bounding a hook](./hooks.md#bounding-a-hook)).
- The **read-only rule** and the `_dirsql_*` **internal-table denial** apply
  identically. A rejected read is an error, not empty output. The read-only
  rule here governs SQL statements; dirsql separately never modifies the
  files it indexes — see
  [Read-only by design](../explanation#read-only-by-design).
- **No per-query timeout.** Unlike the server's 30-second bound (`408`),
  a one-shot query runs to completion — the process *is* the query, so
  cap it from the shell if you want one: `timeout 60 dirsql query "<sql>"`
  (see `timeout(1)`).

#### `--on-file <command>`

Attach a parser to every [path-table](./path-tables.md#parsing-rows-with-on-file)
in the query, so each matched file yields the rows the command prints (a JSON
array of row objects) instead of the stat columns:

```sh
dirsql query "SELECT title, author FROM './posts/*.md'" \
  --on-file 'extract.py {path}'
```

The command follows the [`on-file` hook contract](./hooks.md#on-file) — argv
splitting, `{path}`/`{root}` placeholders, per-file failure isolation, and the
timeout. The parser's output is the whole schema; the stat columns are not
reachable on a parsed path-table. `--on-file` may be given **at most once** (a
repeat is an error pointing at config files) and never touches config-declared
tables. It is a `query`-only flag — server mode rejects it as an unknown
argument. The command string is copy-paste identical to a `[[table]]`
`on-file` key, so an inline parser graduates to a config file unchanged — see
[Parse your files into columns](../howto/parse-files-into-columns.md).

Errors print the same diagnostic the HTTP `{"error": …}` body carries —
config failures, SQL errors, rejected reads, hook failures, timeouts — to
stderr, with exit code `1`.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Query succeeded; rows printed on stdout. |
| `1` | Any failure: config, SQL, rejected read, hook, or timeout. The diagnostic is on stderr. |

## `dirsql init`

Writes a starter `.dirsql.toml` as a scaffold to edit. It does **not**
duplicate the zero-config floor (`SELECT * FROM './'` already lists every file
with no config); instead it shows the **escalation**: one named `[[table]]`
with a glob, a schema, and a real `on-file` hook that pulls structured rows
out of your files.

```bash
dirsql init
```

The output does **not** auto-load. Once you've tweaked it, pass it explicitly
to run against it:

```bash
dirsql "SELECT * FROM files" -c ./.dirsql.toml
```

### Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | current directory | Directory the default `--output` path is resolved against. |
| `--output <path>` | `<root>/.dirsql.toml` | Where to write the config. |
| `--force` | off | Overwrite the output file if it already exists. |

### Requirements and failure modes

All failures exit `1` with a message on stderr:

| Condition | Behavior |
|---|---|
| Output file exists and `--force` not passed | Fails; nothing is written. |
| Output path unwritable (e.g. missing parent directory) | Fails with the underlying I/O error. |

On success, `init` exits `0`.

## Plugins

A **plugin** is an ordinary Python package that ships a `dirsql.toml` config
fragment and declares itself via a `dirsql` entry point. When such a package is
installed in the same environment as `dirsql` (`pip install …`, or
`uvx --with …`), the `uvx`/`pip` launcher **discovers it automatically** and
loads its fragment — its tables are queryable with zero config edits.
Installed = active: there is no enable step and no naming convention. The
fragment is composed *after* your own `-c` configs (so your config takes
precedence in ordering), and the shipped starter `records` table is preserved.

Discovery is **launcher-only** — the standalone `cargo`-installed binary does no
discovery, and the SDKs never auto-discover (pass a plugin's config explicitly
instead). It is **pip/uvx only** for now; the `npx` launcher does not yet
discover.

Turn discovery off with either:

| | Effect |
|---|---|
| `--no-plugin` | Skip plugin discovery for this invocation. Consumed by the launcher; never forwarded to the binary. |
| `DIRSQL_NO_PLUGIN=1` | Same, via the environment. |

A plugin that declares itself but is missing its module or its `dirsql.toml`
fragment is a launcher error naming the package — never a silent skip.
