### Core: internal-table denial covers every schema read

#### Summary

`query()` rejected a schema read of dirsql's internal `_dirsql_*` tables only
when it was a lowercase `PRAGMA`. Any spelling of the name, the `pragma_*`
table-valued functions, and `dbstat` are now denied too. Affects every query
surface (CLI, server, SDKs). Nothing breaks for queries on user tables.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `SELECT ... FROM dbstat` now fails with the internal-table "not authorized"
  error, since it lists every table including the internal ones.
- `pragma_table_info`, `pragma_index_list` and the other `pragma_*` functions
  fail with that error when their argument names a `_dirsql_*` table, in any
  case.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'a\n' > a.md
dirsql "SELECT name FROM pragma_table_info('_dirsql_internal_rows')" --format json
# expected: dirsql query: not authorized: dirsql's internal bookkeeping tables ...
```
