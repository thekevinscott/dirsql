# CLI

The `dirsql` binary has three modes:

| Invocation | Behavior |
|---|---|
| `dirsql` (no subcommand) | Start a long-lived HTTP server exposing a SQL view of a directory. See [HTTP API](./http-api.md). |
| `dirsql query "<sql>"` | Query the file system as JSON |
| `dirsql init` | Generate a `.dirsql.toml` |

## Installation

::: code-group

```bash [npm]
npx dirsql
```

```bash [PyPI]
uvx dirsql
```

```bash [Cargo]
# The `cli` feature is opt-in; this installs the binary only.
cargo install dirsql --features cli
dirsql
```

:::

The npm launcher requires **Node ≥ 20.11**.

## Server mode

```bash
dirsql
# Running at localhost:7117
```

On startup the server prints `Running at <host>:<port>` to stdout. It runs
until it receives `SIGINT` (Ctrl-C) or `SIGTERM`, then drains in-flight
requests, closes open `/events` streams, and exits.

### Flags

| Flag | Default | Description |
|---|---|---|
| `-c, --config <path>` | baked-in default | Path to a [config file](./config.md). **Repeatable** (`-c a -c b`): the configs load and merge in argv order — see [Composing multiple configs](./config.md#composing-multiple-configs). The index is always rooted at the **invocation directory** (the current working directory), regardless of where a config lives — so `--config /elsewhere/.dirsql.toml` still indexes the directory you ran `dirsql` from. With none given, the [baked-in default](#default-mode) `files` table is served — a `./.dirsql.toml` on disk is **not** auto-loaded; pass it explicitly. A `-c` naming a file that does not exist is an [error](#degraded-mode), not a silent fallback to the default. |
| `--host <addr>` | `localhost` | Bind address. |
| `--port <n>` | `7117` | TCP port to bind. |
| `--persist [<path>]` | off | Keep the SQLite index on disk between runs so a restart only re-parses files that actually changed. Bare `--persist` caches at `<root>/.dirsql/cache.db`; `--persist <path>` caches at `<path>`. Off by default (the index is ephemeral). Also available on [`dirsql query`](#dirsql-query), passed after the subcommand. See [Keep the index across restarts](../howto/persist.md). |
| `--extension <path>` | none | Load a SQLite extension by literal path, overriding the config's `[[dirsql.extension]]` entries. Repeatable. Format: `<path>` or `<path>::<entrypoint>`. Internal plumbing for the pip/npm launchers, which resolve package-name extensions and pass the resolved paths here — not intended for direct use. When any `--extension` is present, the config file's own extension entries are not loaded. |
| `--version` | | Print the version and exit. |
| `--help` | | Print usage and exit. |

### Defaults

- Per-query timeout: **30 seconds**. A query exceeding it returns
  `408 Request Timeout`.
- Command hooks (`on-file`, `pre-query`, `post-query`) default to a
  **30-second** timeout each, overridable with the config key
  [`[dirsql].hook-timeout`](./config.md#dirsql-keys).

### Default mode

With no `-c/--config`, the server serves the **baked-in default** — the
shipped config compiled into the binary, indexing the invocation directory
with a single table named `files`. This is a fixed default, *not* a
`.dirsql.toml` read from disk: a `./.dirsql.toml` sitting in the current
directory is **not** auto-loaded (pass it with `-c ./.dirsql.toml` to use
it). The default `files` table:

- Glob: `**/*` — every file under the root, at any depth, no ignores.
- One row per file, with all seven
  [stat columns](./columns.md): `path`, `basename`, `dir`, `ext`,
  `size`, `mtime`, `ctime`.

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT basename, size FROM files ORDER BY size DESC LIMIT 5"}'
```

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
# No -c: the baked-in default `files` table.
dirsql query "SELECT basename, size FROM files ORDER BY size DESC LIMIT 5"
# [{"basename":"model.bin","size":104857600}, …]

# A config table (`posts`) needs its config passed explicitly, AFTER the subcommand.
dirsql query "SELECT COUNT(*) AS n FROM posts" -c ./.dirsql.toml | jq '.[0].n'
```

::: warning Config flags are subcommand-local
Pass `-c`/`--config`, `--persist`, and `--extension` **after** `query`
(`dirsql query "<sql>" -c <cfg>`). A config flag placed *before* the subcommand
is a hard error — `error: the subcommand 'query' cannot be used with
'--config <CONFIG>'` — never silently dropped. (In server mode, with no
subcommand, the same flags are passed directly: `dirsql -c <cfg>`.)
:::

The subcommand builds the index, runs the SQL, prints the result rows as a
JSON array on stdout (byte-identical to the [`POST /query`](./http-api.md)
response body), and exits `0`.

`dirsql query` is a thin adapter over the **same query pipeline the server
uses**, so behavior is identical to `POST /query` by construction:

- **Config discovery** honors `--config` passed after the subcommand (with none
  given, the [baked-in default](#default-mode)), and `--extension` overrides,
  exactly as server mode does.
- **`--persist [<path>]`** is honored, so a repeated `dirsql query` reuses the
  on-disk cache. Because its value is optional, place a bare `--persist` after
  the SQL (`dirsql query "SELECT …" --persist`) or use the `=` form
  (`--persist=/path`) so it does not swallow the SQL argument.
- **Hooks** ([`pre-query`](./hooks.md#pre-query) /
  [`post-query`](./hooks.md#post-query)) and the
  [`[dirsql].hook-timeout`](./config.md#dirsql-keys) apply identically.
- The **30-second query timeout**, the **read-only rule**, and the
  `_dirsql_*` **internal-table denial** apply identically. A rejected read
  is an error, not empty output.

Errors print the same diagnostic the HTTP `{"error": …}` body carries —
config failures, SQL errors, rejected reads, hook failures, timeouts — to
stderr, with exit code `1`.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Query succeeded; rows printed on stdout. |
| `1` | Any failure: config, SQL, rejected read, hook, or timeout. The diagnostic is on stderr. |

## `dirsql init`

Writes a starter `.dirsql.toml` — the same table the [baked-in
default](#default-mode) serves — as a scaffold to edit:

```bash
dirsql init
```

The output does **not** auto-load. Once you've tweaked it, pass it explicitly
to run against it:

```bash
dirsql -c ./.dirsql.toml
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
