# React to file changes

`dirsql` watches the directory it indexes, so your tables update as files
change — and [`GET /events`](../reference/http-api.md#get-events) pushes
every row-level change to you as it happens. No polling, no diffing on your
side.

Row events are emitted for **named tables**, so this flow needs a config —
[path-tables](../reference/path-tables.md) are scanned per query and are not
watched. Define one next to your files. A named table's columns are whatever
its [`on-file`](../reference/config.md#table) hook emits; here a minimal hook
prints each file's basename:

```toml
# .dirsql.toml
[[table]]
ddl     = "CREATE TABLE files (basename TEXT)"
glob    = "**/*"
on-file = '''sh -c 'printf "[{\"basename\":\"%s\"}]" "${1##*/}"' sh {path}'''
```

## 1. Open the stream

With the server running (`npx dirsql server -c ./.dirsql.toml` /
`uvx dirsql server -c ./.dirsql.toml`), subscribe from another terminal:

```bash
curl -N http://localhost:7117/events
```

The server immediately confirms the subscription is attached:

```
event: ready
data: {}
```

## 2. Change a file, receive an event

Create a file under the indexed directory:

```bash
echo 'second' > inbox/two.txt
```

The stream delivers the resulting row change:

```
event: row
data: {"action":"insert","file_path":"inbox/two.txt","old_row":null,"row":{"basename":"two.txt"},"table":"files"}
```

Edits arrive as `update` events carrying both the old and new row;
deletions as `delete`. The full payload schema — including `error` events —
is in [event payloads](../reference/http-api.md#event-payloads).

## Consuming it

Anything that speaks
[Server-Sent Events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events)
works — `EventSource` in a browser, an SSE client library, or plain
`curl -N` in a shell pipeline. Two semantics worth designing around
([stream semantics](../reference/http-api.md#stream-semantics)):

- **Errors don't end the stream.** A malformed file produces an `error`
  event and the stream continues — handle the event, don't reconnect.
- **Slow consumers skip.** A subscriber that falls behind misses the
  overflowed events rather than stalling the server. If you must not miss
  anything, re-query after catching up.

## Notes

- Events reflect *row* changes, not raw filesystem events: a file edit
  that leaves a table's rows identical emits nothing, and one file change
  can emit several events. The diffing model is part of
  [how `dirsql` thinks](../explanation.md).
- Ignored files ([Skip files you don't want indexed](./skip-files.md))
  never generate events.
- Embedding `dirsql` in a program? The SDK's `watch()` yields the same
  events in-process — see the [SDK reference](../reference/sdk.md#watch)
  and [Embed `dirsql` in your application](./embed.md).
