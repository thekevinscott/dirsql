### Core: watch applies a UNIQUE value moving between keyed rows

#### Summary

A watch refresh that permutes a `UNIQUE` column's values across a table's
keyed rows used to fail the batch with `UNIQUE constraint failed`; it now
applies, emitting an `update` per changed row. Affects every watch surface
(server, SDK `watch`/`poll_events`). Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A row whose in-place `UPDATE` collides on a `UNIQUE` value another row is
  giving up in the same edit is deleted and inserted again at the same rowid.
  Its delete and insert triggers fire instead of its update triggers, and an
  `ON DELETE` action on a child table applies to it.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir d
printf '%s\n' '[[table]]' 'name = "t"' 'ddl = "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT UNIQUE)"' 'glob = "d/*.jsonl"' 'on-file = "cat"' > .dirsql.toml
printf '{"id":1,"name":"x"}\n{"id":2,"name":"y"}\n' > d/a.jsonl
dirsql server -c .dirsql.toml --port 7193 &
sleep 1
printf '{"id":1,"name":"y"}\n{"id":2,"name":"x"}\n' > d/a.jsonl
sleep 1.5
curl -s localhost:7193/query -d '{"sql":"SELECT id, name FROM t ORDER BY id"}'
# expected: [{"id":1,"name":"y"},{"id":2,"name":"x"}]
kill %1
```
