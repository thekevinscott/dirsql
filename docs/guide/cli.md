---
outline: deep
---

# CLI

The `dirsql` CLI boots an HTTP server that answers SQL queries over a directory.

The v1 binary is deliberately scoped to a single verb: serving queries over
HTTP. The broader subcommand surface (`init`, `query`, `watch`) from the
roadmap is not yet implemented.

## Usage

```sh
dirsql [DIR] [--config <path>] [--port <N>] [--host <addr>]
```

### Flags

| Flag       | Default                 | Description                                       |
| ---------- | ----------------------- | ------------------------------------------------- |
| `DIR`      | current working dir     | Directory to serve                                |
| `--config` | `<DIR>/.dirsql.toml`    | Path to the config file                           |
| `--port`   | `4321`                  | TCP port to bind                                  |
| `--host`   | `127.0.0.1`             | Host to bind                                      |

## Routes

### `POST /query`

Request body: JSON `{ "sql": "SELECT ..." }`.

Responses:

- `200 OK` -- `{ "rows": [ { "col": value, ... }, ... ] }`
- `400 Bad Request` -- `{ "error": "..." }` for engine errors or malformed
  request bodies.

Rows are JSON objects keyed by column name. Core `Value` variants map to JSON as:

| Rust `Value`    | JSON                                |
| --------------- | ----------------------------------- |
| `Null`          | `null`                              |
| `Integer(i64)`  | number                              |
| `Real(f64)`     | number (non-finite -> `null`)       |
| `Text(String)`  | string                              |
| `Blob(Vec<u8>)` | `{ "$blob_b64": "<base64>" }`       |

### `GET /healthz`

Returns `200 OK` with body `ok`. Cheap probe for readiness.

### `GET /events`

Returns `501 Not Implemented`. The streaming events API (file-change driven
row events from `DirSQL::watch`) is tracked as a follow-up; the wire format
(SSE vs WebSocket vs JSONL) has not been chosen. This route is pinned so it
cannot silently regress.

## Example

```sh
# in a directory containing a .dirsql.toml:
dirsql --port 4321

# from anywhere else:
curl -sX POST http://127.0.0.1:4321/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT * FROM items LIMIT 5"}'
```

## Exit behavior

- Missing config file -> non-zero exit, message on stderr naming the
  expected path.
- Bind failure (port in use) -> non-zero exit with the bind error.
- SIGINT (`Ctrl-C`) -> graceful shutdown.
