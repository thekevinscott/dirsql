---
canonical: https://thekevinscott.github.io/dirsql/guide/cli
---

# Command-Line Interface

> Online: <https://thekevinscott.github.io/dirsql/guide/cli>

`dirsql` starts an HTTP server that exposes identical SDK functionality.

## Installation

::: code-group

```bash [npm]
npx dirsql
```

```bash [PyPI]
uvx dirsql
```

```bash [Cargo]
# Installs the binary only (non-default feature)
cargo install dirsql --features cli
dirsql
```

:::

::: tip For Rust library consumers
The `cli` feature is **opt-in**. Adding `dirsql` as a library dependency (`cargo add dirsql`) pulls no CLI dependencies — only the core library. See the [Rust library README](https://github.com/thekevinscott/dirsql/tree/main/packages/rust) for details.
:::

## Running the server

Run `dirsql` from the directory containing your files:

```bash
dirsql

$ Running at localhost:7117
```

### Flags

| Flag | Default | Description |
|---|---|---|
| `--config <path>` | `./.dirsql.toml` | Path to the config file. The index is rooted at the directory containing this file. |
| `--host <addr>` | `localhost` | Bind address |
| `--port <n>` | `7117` | TCP port to bind |

## HTTP API

### `POST /query`

Run a SQL query. Request body is JSON:

```json
{"sql": "SELECT title, author FROM posts WHERE draft = 0"}
```

Response is a JSON array of row objects:

```json
[
  {"title": "Hello World", "author": "alice"},
  {"title": "Second Post", "author": "bob"}
]
```

On error, the server returns a non-2xx status with a JSON body:

```json
{"error": "syntax error near \"SLECT\""}
```

Malformed SQL returns `400`, not `500` — the client sent bad input. Missing / unreadable config returns `503`.

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT COUNT(*) AS n FROM posts"}' \
  | jq
```

### `GET /events`

Opens a [Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events) stream of change events. Each `data:` payload is the same JSON schema the SDK emits from [`db.watch()`](./watching.md#event-types):

```
event: row
data: {"action":"insert","table":"posts","file_path":"posts/hello.json","row":{"title":"Hello World","author":"alice"},"old_row":null}

event: row
data: {"action":"update","table":"posts","file_path":"posts/hello.json","row":{"title":"Hello, world","author":"alice"},"old_row":{"title":"Hello World","author":"alice"}}

event: row
data: {"action":"delete","table":"posts","file_path":"posts/second.json","row":{"title":"Second Post","author":"bob"},"old_row":null}
```

Errors during extraction appear as `{"action":"error",...}` events on the same stream. They do **not** terminate the stream — a malformed file is a per-event problem, not a server-wide one.

```bash
curl -N http://localhost:7117/events
```

## Piping event streams

The SSE stream is easy to tee into shell tools with `curl -N` plus `jq`:

```bash
# Log every delete to a file
curl -N http://localhost:7117/events \
  | jq -cR 'fromjson? | select(.action=="delete")' \
  >> deletes.log

# Alert on errors
curl -N http://localhost:7117/events \
  | jq -c 'fromjson? | select(.action=="error")' \
  | while read -r line; do notify-send "dirsql error" "$line"; done
```

(The `fromjson?` wrapping strips the `data:` framing; drop it if your SSE client is already parsing frames.)
