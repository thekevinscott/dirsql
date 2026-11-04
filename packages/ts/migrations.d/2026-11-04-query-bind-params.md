### Bind parameters in `query`

#### Summary

`await db.query(sql, params)` and `await db.queryOrdered(sql, params)` bind values to the statement's placeholders through SQLite rather than the SQL text. Existing calls without params keep working. Nothing breaks.

#### Required changes

_None._ Hand-quoted SQL keeps working; to bind instead:

| Before | After |
|---|---|
| ``await db.query(`... WHERE a = ${quote(text)}`)`` | `await db.query("... WHERE a = :a", { a: text })` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

`await new DirSQL({ root: "." }).query("SELECT :x AS x", { x: "it's" })` resolves to `[{ x: "it's" }]`.
