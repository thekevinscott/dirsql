**Fixed**

A file the scan cannot index no longer costs the files around it, and a partial
index is now visible at the exit code instead of passing for a complete one
(#714).

Two failure modes behaved inconsistently. A hook that exited non-zero or emitted
malformed output was already skipped — `run_on_file` logged it and returned an
empty row set — but a row that failed **strict** normalization aborted the whole
scan, so one bad column cost every other file's rows. Both are now the same
thing: the file is skipped, the scan commits what it could index, and the
failure is recorded.

- `DirSQL::scan_failures()` returns the files the scan could not index, each
  with its root-relative path and the hook's own message. Empty for a clean
  scan.
- The CLI reports skips on **stderr**, at most ten by name followed by
  `... and N more`, and exits **23** — rsync's "partial transfer due to error" —
  when any file was skipped. stdout still carries only the query result, so
  `dirsql "SELECT …" | jq` keeps working and can now tell a partial index from a
  complete one. A clean scan still exits `0`; a failed run still exits `1`.
- `run_on_file` returns `Result` rather than logging and flattening to an empty
  row set, so a skip and a file that legitimately produced no rows are no longer
  indistinguishable.

SQLite, DDL and extension errors are unchanged: they still abort the scan and
roll back. Only per-file hook outcomes became recoverable.

A skipped file never reaches the persistent file index, so it is retried on the
next scan — the cache is incomplete, never wrong.

**Removed**

`DirSqlError::OnFile` and `DirSqlError::OnFileMany`. Nothing can produce them
now that a per-file failure is not a scan error.
