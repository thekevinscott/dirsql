# HTTP API

The [`dirsql` server](./cli.md#server-mode) (default `localhost:7117`)
exposes two endpoints: `POST /query` and `GET /events`.

## `POST /query`

Run a read-only SQL query. Request body is JSON:

```json
{"sql": "SELECT title, author FROM posts WHERE draft = 0"}
```

The `200` response is a JSON array of row objects keyed by column name:

```json
[
  {"title": "Hello World", "author": "alice"},
  {"title": "Second Post", "author": "bob"}
]
```

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT COUNT(*) AS n FROM files"}' \
  | jq
```

### Value serialization

| SQLite value | JSON |
|---|---|
| `NULL` | `null` |
| `INTEGER` | number |
| `REAL` | number; `NaN` / `±Infinity` become `null` |
| `TEXT` | string |
| `BLOB` | lowercase hex string (e.g. `"deadbeef"`) |

Internal tracking columns (`_dirsql_file_path`, `_dirsql_row_index`) are
excluded from `SELECT *` results.

### Status codes

| Status | When |
|---|---|
| `200` | Query succeeded. Body: array of row objects (or the [`post-query`](./hooks.md#post-query) hook's JSON). |
| `400` | Malformed JSON body, missing or empty `sql` field, or a SQL error (syntax error, unknown table, or a statement SQLite classifies as a write — queries are read-only). |
| `405` | `GET /query`. Plain-text body `method not allowed`. |
| `408` | The query exceeded the 30-second per-query timeout. |
| `500` | Internal server fault, or a failed [`pre-query`](./hooks.md#pre-query) / [`post-query`](./hooks.md#post-query) hook. |
| `503` | The server is in [degraded mode](./cli.md#degraded-mode) (the config file exists but failed to load). |

All error responses (except `405`) are `application/json`:

```json
{"error": "syntax error near \"SLECT\""}
```

### Hook interactions

- With [`[dirsql].pre-query`](./config.md#dirsql-keys) configured, the
  request body is **not** parsed as `{"sql": …}`; the raw body is passed to
  the hook, which prints the SQL to run. Hook failure → `500`.
- With [`[dirsql].post-query`](./config.md#dirsql-keys) configured, the
  `200` body is whatever JSON the hook prints instead of the bare row
  array. Hook failure or non-JSON output → `500`.

## `GET /events`

Opens a [Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events)
stream of row-change events.

```bash
curl -N http://localhost:7117/events
```

On open, the server emits a single `ready` frame signalling the
subscription is attached. Every subsequent frame is named `row`:

```
event: ready
data: {}

event: row
data: {"action":"insert","table":"posts","file_path":"posts/hello.json","row":{"title":"Hello World"},"old_row":null}
```

### Event payloads

The `data:` payload is one JSON object per row-level change:

| Field | `insert` | `update` | `delete` | `error` |
|---|---|---|---|---|
| `action` | `"insert"` | `"update"` | `"delete"` | `"error"` |
| `table` | table name | table name | table name | table name, or `null` when the failure isn't tied to one table |
| `file_path` | path relative to the root | same | same | same |
| `row` | the new row | the new row | the deleted row | — (absent) |
| `old_row` | `null` | the previous row | `null` | — (absent) |
| `error` | — | — | — | message string |

Row values serialize as in [`POST /query`](#value-serialization).

### Stream semantics

- **Errors do not terminate the stream.** A malformed file produces an
  `error` event; the stream continues.
- **Slow consumers skip, not crash.** A subscriber that lags behind the
  event buffer silently misses the overflowed events and keeps receiving
  new ones.
- The server sends periodic SSE keep-alive comments.
- The stream closes when the server shuts down.

### Status codes

| Status | When |
|---|---|
| `200` | Stream opened (`text/event-stream`). |
| `405` | `POST /events`. Plain-text body `method not allowed`. |
| `503` | The server is in [degraded mode](./cli.md#degraded-mode). JSON `{"error": …}` body. |
