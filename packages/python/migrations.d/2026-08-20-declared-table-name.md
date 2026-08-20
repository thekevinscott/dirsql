### `Table` and `[[table]]` require an explicit `name` (#962)

#### Summary

`Table` previously derived its SQL name by parsing the `CREATE TABLE` head of
`ddl`, exposing the result as an `Optional[str]` attribute. The name is now
**declared**: `Table` takes a required keyword-only `name`, and a `[[table]]`
entry in a `.dirsql.toml` requires a `name` key. Whether the `ddl` creates a
table by that name is checked against SQLite's catalog at load time.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| `Table(ddl=..., glob=..., on_file=...)` | `TypeError: Table.__new__() missing 1 required keyword argument: 'name'` | Pass `name="<table>"`, matching the table the `ddl` creates. |
| `[[table]]` with `ddl` + `glob` + `on-file` | `await db.ready()` raises: `Missing required field 'name' in [[table]] entry` | Add `name = "<table>"` to the entry. |
| `table.name` may be `None` | `table.name` is the declared `str` | Drop any `if table.name is None` handling. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A `[[table]]` whose `ddl` creates a table under a different name than its
  `name` now raises at `ready()` with an error naming the entry
  (`table 'messages': …`), before any file is ingested.
- Quoted (`CREATE TABLE "messages"`), schema-qualified and `IF NOT EXISTS` DDL
  keep working unchanged: SQLite records the bare identifier, so a plain
  `name="messages"` matches.

#### Verification

```python
import asyncio, json
from dirsql import DirSQL, Table

db = DirSQL(
    "./data-root",
    tables=[
        Table(
            name="records",
            ddl="CREATE TABLE records (id TEXT)",
            glob="data/*.json",
            on_file=lambda path: json.load(open(path, encoding="utf-8")),
        )
    ],
)
asyncio.run(db.ready())
# -> builds; `SELECT id FROM records` returns the file's rows.
# Omitting `name=` raises TypeError; a `name` the ddl never creates raises at ready().
```
