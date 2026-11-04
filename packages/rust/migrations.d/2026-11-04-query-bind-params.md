### Bind parameters in `query`

#### Summary

`DirSQL::query_with(sql, &Params)` (plus `query_ordered_with`, `query_each_with`, `AsyncDirSQL::query_with`) binds values to the statement's placeholders through SQLite rather than the SQL text. Existing calls without params keep working. Nothing breaks.

#### Required changes

_None._ Hand-quoted SQL keeps working; to bind instead:

| Before | After |
|---|---|
| `db.query(&format!("... WHERE a = {}", quote(text)))` | `db.query_with("... WHERE a = ?", &Params::Positional(vec![Value::Text(text)]))` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query whose placeholders get no values still fails, but with dirsql's message (`the query has 1 parameter(s), but 0 value(s) were given`) instead of SQLite's `Wrong number of parameters passed to query`.

#### Verification

`db.query_with("SELECT ? AS x", &Params::Positional(vec![Value::Integer(1)]))` returns one row with `x` = `Value::Integer(1)`.
