---
canonical: https://thekevinscott.github.io/dirsql/cli/http-api
---

# HTTP API

> Online: <https://thekevinscott.github.io/dirsql/cli/http-api>

Once the [server is running](./server.md), `dirsql` exposes two HTTP
endpoints: `POST /query` for SQL queries and `GET /events` for a real-time
change stream.

## `POST /query`

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

Malformed SQL returns `400`. An unreadable or malformed config returns `503`; a *missing* config is not an error — the server serves the default `files` table.

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT COUNT(*) AS n FROM files"}' \
  | jq
```

The query interface is the same one the SDK exposes; see [Querying](../guide/querying.md)
for SQL semantics, the read-only restriction, and the return format.

## `GET /events`

Opens a [Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events) stream of change events.

On stream open the server emits a single `ready` frame so clients have a reliable signal that the subscription is attached. Every subsequent frame is named `row`, and its `data:` payload is the same JSON schema the SDK emits from [`db.watch()`](../guide/watching.md#event-types):

```
event: ready
data: {}

event: row
data: {"action":"insert","table":"posts","file_path":"posts/hello.json","row":{"title":"Hello World","author":"alice"},"old_row":null}

event: row
data: {"action":"update","table":"posts","file_path":"posts/hello.json","row":{"title":"Hello, world","author":"alice"},"old_row":{"title":"Hello World","author":"alice"}}

event: row
data: {"action":"delete","table":"posts","file_path":"posts/second.json","row":{"title":"Second Post","author":"bob"},"old_row":null}
```

Errors during extraction appear as `error` action events on the same `row` stream. An error event carries an `error` message string and does **not** include `row` or `old_row`:

```
event: row
data: {"action":"error","table":"posts","file_path":"posts/broken.json","error":"Extract error: ..."}
```

(`table` is `null` when the failure isn't tied to a specific table.) Errors do **not** terminate the stream — a malformed file is a per-event problem, not a server-wide one.

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
