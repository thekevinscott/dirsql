### Core: path-table equality on `path`, `basename` or `dir` is answered by lookup

#### Summary

A `dirsql_path` table now tells SQLite it can answer an equality constraint
on `path`, `basename` or `dir` at a lower cost than a scan, and serves it
from a lookup built on the statement's one walk. No signature, name, config
key or CLI flag changes; what moves is the query plan SQLite picks for a join
between two path-tables or a `WHERE` equality on one of those columns.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A join of two path-tables on `path`, `basename` or `dir` plans the inner
  table as a lookup (`SCAN t VIRTUAL TABLE INDEX 3:` under `EXPLAIN QUERY
  PLAN` for `dir`) instead of rescanning it per outer row. The rows, their
  order under `ORDER BY`, and their `rowid`s are unchanged.
- An equality whose right-hand side is not text (`basename = 5`, `dir = NULL`)
  selects nothing, as it did under a scan; SQLite re-checks every equality it
  hands over, so an answer never widens.
- `dirsql_parsed` tables keep their scan-only plan.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir p1 p2
printf 'one\n' > p1/title.md
printf 'x\n' > p1/abstract.md
printf 'x\n' > p2/abstract.md
dirsql query "SELECT a.dir, t.content AS title FROM './*/abstract.md' a JOIN './*/title.md' t ON t.dir = a.dir"
# expected: [{"dir":"p1","title":"one\n"}]
dirsql query "EXPLAIN QUERY PLAN SELECT a.dir FROM './*/abstract.md' a JOIN './*/title.md' t ON t.dir = a.dir" | grep -o 'SCAN t VIRTUAL TABLE INDEX 3:'
# expected: SCAN t VIRTUAL TABLE INDEX 3:
```
