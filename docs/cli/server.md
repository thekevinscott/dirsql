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

The server reads tables from a [config file](./config.md). By default it
looks for `./.dirsql.toml`; pass `--config <path>` to point at a different
`.toml` file.

## Defaults

If no config file is found, `dirsql` serves a single table named `files`, with one row per file under the directory

```bash
cd ~/some/directory   # no .dirsql.toml here
dirsql

$ Running at localhost:7117
```

```bash
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT _basename, _size FROM files ORDER BY _size DESC LIMIT 5"}'
```

A config file will override the default.

## Flags

| Flag | Default | Description |
|---|---|---|
| `--config <path>` | `./.dirsql.toml` | Path to the `.toml` [config file](./config.md). The index is rooted at the directory containing this file. |
| `--host <addr>` | `localhost` | Bind address |
| `--port <n>` | `7117` | TCP port to bind |

Once the server is running, see the [HTTP API](./http-api.md) for the request
and response formats.
