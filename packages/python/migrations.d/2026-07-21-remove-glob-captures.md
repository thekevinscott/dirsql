### Glob captures removed; colliding placeholder + column is a load-time error (#655)

#### Summary

Glob `{name}` placeholders no longer populate columns. `{name}` still matches
like `*`, so the set of matched files is unchanged, but a placeholder whose
name is also a declared DDL column now raises at load — surfaced through
`await DirSQL.ready()` (or the first `await` on a query that triggers the
build). A placeholder with no matching column keeps working silently. To keep
a column that used to come from a capture, split the value out of the path in
your `on_file` hook.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `Table` / config whose glob placeholder names a declared column | `Table(ddl="CREATE TABLE comments (thread_id TEXT)", glob="_comments/{thread_id}/*.txt", on_file=...)` (relied on the capture) | `Table(ddl="CREATE TABLE comments (thread_id TEXT)", glob="_comments/*/*.txt", on_file=lambda p: [{"thread_id": p.split(os.sep)[-2]}])` (hook splits the path) |
| Placeholder with no matching column | `glob="_comments/{thread_id}/*.txt"` with no `thread_id` column | Unchanged — still matches like `*`. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A column previously populated from a `{name}` capture now reads `NULL`; if
  the placeholder name matches the column name, `ready()` raises instead of
  silently NULLing the column. The exception message names the placeholder and
  the fix.
- Matching is unchanged: `{name}` behaves like `*`.

#### Verification

```python
import asyncio, os
from dirsql import DirSQL, Table

async def main():
    db = DirSQL(
        "/data",
        tables=[Table(
            ddl="CREATE TABLE comments (thread_id TEXT, basename TEXT)",
            glob="_comments/*/*.txt",
            on_file=lambda p: [{"thread_id": p.split(os.sep)[-2]}],
        )],
    )
    await db.ready()
    print(await db.query("SELECT thread_id FROM comments"))
    # expected: rows carrying the directory segment as thread_id.

asyncio.run(main())
```
