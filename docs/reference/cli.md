# CLI

The `dirsql` binary has three modes:

| Invocation | Behavior |
|---|---|
| `dirsql` (no subcommand) | Start a long-lived HTTP server exposing a SQL view of a directory. See [HTTP API](./http-api.md). |
| `dirsql query "<sql>"` | Build the index, run one query, print the rows as JSON, exit. No server, no watch. |
| `dirsql init` | Write a starter `.dirsql.toml` — the same default `files` table zero-config mode serves. |

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
| `--config <path>` | `./.dirsql.toml` | Path to the [config file](./config.md). The index is rooted at the directory containing this file (unless the config sets `[dirsql].root`). When the file does not exist, the server runs in [zero-config mode](#zero-config-mode). |
| `--host <addr>` | `localhost` | Bind address. |
| `--port <n>` | `7117` | TCP port to bind. |
| `--extension <path>` | none | Load a SQLite extension by literal path, overriding the config's `[[dirsql.extension]]` entries. Repeatable. Format: `<path>` or `<path>::<entrypoint>`. Internal plumbing for the pip/npm launchers, which resolve package-name extensions and pass the resolved paths here — not intended for direct use. When any `--extension` is present, the config file's own extension entries are not loaded. |
| `--version` | | Print the version and exit. |
| `--help` | | Print usage and exit. |

### Defaults

- Per-query timeout: **30 seconds**. A query exceeding it returns
  `408 Request Timeout`.
- Command hooks (`on-file`, `pre-query`, `post-query`) default to a
  **30-second** timeout each, overridable with the config key
  [`[dirsql].hook-timeout`](./config.md#dirsql-keys).

### Zero-config mode

When the config file named by `--config` does not exist, the server indexes
the directory that would have contained it (the current directory for the
default `./.dirsql.toml`) with a single table named `files`:

- Glob: `**/*` — every file under the root, at any depth, no ignores.
- One row per file, with all seven
  [stat columns](./columns.md): `path`, `basename`, `dir`, `ext`,
  `size`, `mtime`, `ctime`.

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT basename, size FROM files ORDER BY size DESC LIMIT 5"}'
```

A config file, when present, fully overrules this default. A *missing*
config is not an error; only a config that exists but fails to load is (see
below).

### Degraded mode

When the config file exists but cannot be resolved or loaded (unreadable
path, invalid TOML, schema errors), the server still starts and binds, but
every request to `/query` and `/events` returns `503 Service Unavailable`
with a JSON body describing the failure:

```json
{"error": "failed to load config: ..."}
```

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Clean shutdown after `SIGINT` / `SIGTERM`. |
| `1` | Failed to bind `host:port`, or an error during shutdown. |

## `dirsql query`

Run one SQL query from the shell — for ad-hoc inspection, scripting, and
docs verification snippets — without booting the server and `curl`ing it:

```bash
dirsql query "SELECT basename, size FROM files ORDER BY size DESC LIMIT 5"
# [{"basename":"model.bin","size":104857600}, …]

dirsql query "SELECT COUNT(*) AS n FROM posts" | jq '.[0].n'
```

The subcommand builds the index, runs the SQL, prints the result rows as a
JSON array on stdout (byte-identical to the [`POST /query`](./http-api.md)
response body), and exits `0`.

`dirsql query` is a thin adapter over the **same query pipeline the server
uses**, so behavior is identical to `POST /query` by construction:

- **Config discovery** honors `--config` (default `./.dirsql.toml`),
  [zero-config mode](#zero-config-mode), and `--extension` overrides,
  exactly as server mode does.
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

Writes a starter `.dirsql.toml`: the exact single `files` table
[zero-config mode](./config.md) serves, over every file in the directory
using the [stat columns](./columns.md) — never content-derived columns.
`init` does not inspect the target directory; the written content is fixed,
so the two surfaces can never drift apart. No LLM, no network.

```bash
dirsql init
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
