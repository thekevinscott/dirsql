### Queries read what is on disk

#### Summary

A query on a long-lived Rust or Python `DirSQL` brings its named tables in line with the files on disk before reading. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query re-parses the files that changed since the index last read them, so code that waited on a watch event or slept before querying no longer needs to. A change a query reads first still arrives on `watch()` and `poll_events()`, once.

Each query walks its tables' globs and stats every match, so it costs about what the same query on a path-table costs. When a file on disk breaks a foreign key, a query fails with the same `ForeignKeyViolation` a build over that tree returns, until the file is fixed.

#### Verification

Build a `DirSQL` over a directory with a declared table, write a matching file, then query the table with no wait; the row for the new file is in the result.
