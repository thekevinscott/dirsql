### Bind parameters in `query`

#### Summary

`await db.query(sql, params)` binds values to the statement's placeholders through SQLite rather than the SQL text. Existing calls without params keep working. Nothing breaks.

#### Required changes

_None._ Hand-quoted SQL keeps working; to bind instead:

| Before | After |
|---|---|
| `await db.query(f"... WHERE a = {quote(text)}")` | `await db.query("... WHERE a = :a", {"a": text})` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query whose placeholders get no values still fails, but with dirsql's message (`the query has 1 parameter(s), but 0 value(s) were given`) instead of SQLite's `Wrong number of parameters passed to query`.

#### Verification

`await DirSQL(".").query("SELECT :x AS x", {"x": "it's"})` returns `[{"x": "it's"}]`.
