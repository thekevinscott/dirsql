---
canonical: https://thekevinscott.github.io/dirsql/cli/server
---

# Running the Server

> Online: <https://thekevinscott.github.io/dirsql/cli/server>

The `dirsql` CLI is an HTTP server that exposes identical SDK functionality
over [`POST /query`](./http-api.md#post-query) and [`GET /events`](./http-api.md#get-events).

## Subcommands

| Command | Purpose |
|---|---|
| `dirsql` (no subcommand) | Start the long-lived HTTP server (default behavior, see below). |
| `dirsql init` | Generate a starter `.dirsql.toml` from the contents of a directory. See [Generating a Config](./init.md). |

## Running the server

Run `dirsql` from the directory containing your files:

```bash
dirsql

$ Running at localhost:7117
```

The server reads tables from a [`.dirsql.toml`](./config.md) config file. By
default it looks for `./.dirsql.toml`; override the path with `--config`.

## Zero-config mode

If no config file is found, `dirsql` still starts and serves a single
built-in table named `files` -- one row per file under the directory, with
the filesystem-fact columns `_path`, `_basename`, `_dir`, `_ext`, `_size`,
`_mtime`, and `_ctime`. No ignores are applied: every file under the
directory is indexed.

```bash
cd ~/some/directory   # no .dirsql.toml here
dirsql

$ Running at localhost:7117
```

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT _basename, _size FROM files ORDER BY _size DESC LIMIT 5"}'
```

This makes `dirsql` useful immediately in any directory. A `.dirsql.toml`,
when present, fully overrules the default: only the tables it declares are
served, and the `files` table is not added unless the config declares it.
Run [`dirsql init`](./init.md) to generate a starter config.

## Flags

| Flag | Default | Description |
|---|---|---|
| `--config <path>` | `./.dirsql.toml` | Path to the config file. The index is rooted at the directory containing this file. |
| `--host <addr>` | `localhost` | Bind address |
| `--port <n>` | `7117` | TCP port to bind |

Once the server is running, see the [HTTP API](./http-api.md) for the request
and response formats.
