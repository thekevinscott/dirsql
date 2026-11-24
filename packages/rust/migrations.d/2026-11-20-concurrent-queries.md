### Queries run concurrently after the first overlap

#### Summary

A `DirSQL` instance answers queries concurrently once one query has had to wait for another. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- The query that first has to wait for another still waits, then the index is copied to `dirsql-<pid>-<n>.db` in SQLite's temp directory (`SQLITE_TMPDIR`, then `TMPDIR`, then `/var/tmp`, `/usr/tmp`, `/tmp`); queries and watch batches wait for that copy. From then on a cheap query is answered while a slow one runs, and a watch batch lands while a query runs without that query reading it.
- The file and its `-wal` and `-shm` companions are deleted on shutdown; a process killed before that leaves them behind.
- Queries that name a batched function still run one at a time.

#### Verification

Start `dirsql server`, send a slow query and, while it runs, a cheap one: the cheap one waits. Repeat: the cheap one now returns at once, and `ls "${SQLITE_TMPDIR:-${TMPDIR:-/var/tmp}}"/dirsql-*.db` lists the server's index file.
