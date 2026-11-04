### A failing `on_file` callback fails the build and the query

#### Summary

In every SDK (Rust, Python, TypeScript), a per-file `on_file` callback that raises, or returns a row the table rejects, now fails the build: `ready()` and every query raise `` table `<table>`, file `<path>`: <original error> ``. A file re-read at query time that fails, fails that query. Before, a raising callback's file was skipped and the build succeeded with that file's rows missing; a `NOT NULL` or `UNIQUE` rejection already failed the build, but with a bare SQLite error naming no file. This breaks code that relied on the skip.

#### Required changes

| Before | After |
|---|---|
| Rely on a raising `on_file` to skip a file, then read `scan_failures()` | Return `[]` from `on_file` for a file that should contribute no rows; `scan_failures()` is always empty |
| Writer writes a watched file in place | Writer writes to a temporary file in the same directory and renames it into place, so a query never reads a half-written file |
| Rust: exhaustive `match` on `DirSqlError` | Add an arm for `DirSqlError::OnFile { table, path, message }` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A callback that raises (for example `KeyError` on a missing key) now makes `ready()` and every query raise instead of returning the other files' rows. A file that changes on disk into one the callback cannot parse makes the next query raise rather than drop that file's rows; the query succeeds again once the file parses.

#### Verification

```python
import asyncio, json, os
from dirsql import DirSQL, Table
os.makedirs("d", exist_ok=True)
open("d/a.json", "w").write('{"m": 1}')
db = DirSQL(".", tables=[Table(name="t", ddl="CREATE TABLE t (n INTEGER)", glob="d/*.json",
                               on_file=lambda p: [{"n": json.load(open(p))["n"]}])])
asyncio.run(db.ready())
```

raises ``RuntimeError: table `t`, file `d/a.json`: KeyError: 'n'``.
