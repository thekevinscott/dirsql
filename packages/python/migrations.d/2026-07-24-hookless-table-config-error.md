### Hook-less `[[table]]` is now a config error (#634)

#### Summary

A `[[table]]` with no `on-file` hook previously loaded fine and — after the
stat-fact injection layer was removed — produced rows that were all-NULL.
Loading such a config through the Python SDK (`DirSQL(config=…)`, awaited via
`.ready()`) now raises. No API signature changes — this is a load-time
validation change surfaced from the core.

#### Required changes

| Before (loaded, all-NULL rows) | After (raises) | Fix |
| ------------------------------ | -------------- | --- |
| `.dirsql.toml` `[[table]]` with `ddl` + `glob` but no `on-file` | `.ready()` raises `[[table]] '<glob>' has no on-file hook …` | Add an `on-file` hook that emits the declared columns. |
| You only wanted stat columns and wrote no hook code | `.ready()` raises | Drop the `[[table]]` and query the path directly: `SELECT path, size, mtime FROM './'` (globbable: `FROM './**/*.md'`). |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `[[table]]` with no `on-file` hook, which used to load (and, after injection
  removal, yield all-NULL rows), now raises at config load, with an error naming
  the offending glob and pointing at the `FROM './'` path-table replacement.

#### Verification

```python
import asyncio
from dirsql import DirSQL

# .dirsql.toml with a hook-less table:
#   [[table]]
#   ddl  = "CREATE TABLE files (path TEXT, size INTEGER)"
#   glob = "**/*.md"

db = DirSQL(config=".dirsql.toml")
# Before this change: await db.ready() succeeded; SELECT * FROM files -> all-NULL rows.
# After this change:
try:
    asyncio.run(db.ready())
except Exception as e:
    print(e)  # -> ... has no on-file hook ... query the path directly: `FROM './'`

# Replacement with no hook code — query the path directly (no config):
db2 = DirSQL(root=".")
asyncio.run(db2.ready())
print(asyncio.run(db2.query("SELECT path, size FROM './**/*.md'")))
```
