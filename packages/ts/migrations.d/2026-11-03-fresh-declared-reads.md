### Queries read what is on disk

#### Summary

`db.query()` and `db.queryOrdered()` bring the named tables in line with the files on disk before running. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query re-runs `onFile` on the files that changed since the index last read them, so code that waited on a watch event or slept before querying no longer needs to. A change a query reads first still arrives on `watch()`, once.

The re-reads run on the JavaScript thread, because `onFile` can only run there: each query walks its tables' globs and stats every match before the SQL leaves for the threadpool, about what the same query on a path-table costs. When a file on disk breaks a foreign key, the query rejects with the error a build over that tree returns, until the file is fixed.

#### Verification

Build a `DirSQL` over a directory with a declared table, write a matching file, then `await db.query(...)` the table with no wait; the row for the new file is in the result.
