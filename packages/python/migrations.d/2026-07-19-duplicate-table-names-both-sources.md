### Duplicate-table error message now names both definition sites

#### Summary

Registering two tables with the same name has always failed. The message named
only the table (`duplicate table name: dup`), so a collision between a
programmatic table and a config-defined one gave no pointer to either
definition. The message now names the table and both sources. No API changed —
the raised type is the same; only the text differs.

#### Required changes

| Surface | Before | After |
|---|---|---|
| Message (mixed origins) | `duplicate table name: dup` | `Table 'dup' is defined by both a programmatic table and config /proj/dirsql.toml` |
| Message (same origin) | `duplicate table name: dup` | `Table 'dup' is defined twice by a programmatic table` |

Code that asserts on the **string** `"duplicate table name"` must match the new
wording. Matching on message text is discouraged; catch the exception type
instead.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Which collisions are detected is unchanged; only the diagnostic improved.
- A collision whose two sides share an origin reads `defined twice by <source>`
  instead of naming the same source twice.

#### Verification

```python
import asyncio
from dirsql import DirSQL, Table

db = DirSQL(".", tables=[
    Table(ddl="CREATE TABLE dup (a TEXT)", glob="**/*.a", on_file=lambda p: []),
    Table(ddl="CREATE TABLE dup (b TEXT)", glob="**/*.b", on_file=lambda p: []),
])
try:
    asyncio.run(db.ready())
except Exception as err:
    print(err)
```

Expected:

```
Table 'dup' is defined twice by a programmatic table
```
