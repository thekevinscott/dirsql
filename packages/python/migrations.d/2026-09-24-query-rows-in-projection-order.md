### `db.query()` rows are keyed in projection order

**Summary**

Each dict `db.query()` returns is now keyed in the order the SELECT list names the columns; `SELECT *` follows the table's declared column order. The keys previously came out in the core's hash-map order, which varied from process to process. No signature changed.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Call | Before | After |
| --- | --- | --- |
| `await db.query("SELECT path, size FROM './'")` | keys in arbitrary order, e.g. `{'size': 6, 'path': 'a.md'}` | `{'path': 'a.md', 'size': 6}` |

The old order was never stable, so no caller could have depended on it. Code that sorted the keys to get a stable order can drop the sort if it wants the order the query asked for.

**Verification**

```python
import asyncio
from dirsql import DirSQL

async def main():
    db = DirSQL(".")
    print(await db.query("SELECT size, path FROM './' LIMIT 1"))

asyncio.run(main())
```

Expected: `[{'size': ..., 'path': ...}]`, `size` first as written.
