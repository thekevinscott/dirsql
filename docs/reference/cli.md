# CLI

The `dirsql` binary has two modes:

| Invocation | Behavior |
|---|---|
| `dirsql` (no subcommand) | Start a long-lived HTTP server exposing a SQL view of a directory. See [HTTP API](./http-api.md). |
| `dirsql init` | Generate a starter `.dirsql.toml` by running `claude` over a directory. |

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
  [virtual columns](./columns.md): `_path`, `_basename`, `_dir`, `_ext`,
  `_size`, `_mtime`, `_ctime`.

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT _basename, _size FROM files ORDER BY _size DESC LIMIT 5"}'
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

## `dirsql init`

Generates a `.dirsql.toml` by running the `claude` CLI over the target
directory. The generated config contains only filesystem-fact tables
(`[[table]]` entries whose columns come from [glob captures and virtual
columns](./columns.md)) — never content-derived columns.

```bash
dirsql init
```

### Flags

| Flag | Default | Description |
|---|---|---|
| `--root <path>` | current directory | Directory to scan. |
| `--output <path>` | `<root>/.dirsql.toml` | Where to write the generated config. |
| `--force` | off | Overwrite the output file if it already exists. |

### Requirements and failure modes

`init` requires `claude` on `PATH`, signed in; there is no separate API key.
All failures exit `1` with a message on stderr:

| Condition | Behavior |
|---|---|
| Output file exists and `--force` not passed | Fails before invoking `claude` (no LLM call is made). |
| `claude` not found on `PATH` | Fails with a pointer to the Claude Code install docs. |
| `claude` exits non-zero | Fails with `claude`'s stderr; no partial config is written. |
| `claude` produces non-UTF-8 output | Fails; nothing is written. |

On success, `claude`'s stdout is written verbatim to the output path and
`init` exits `0`.
