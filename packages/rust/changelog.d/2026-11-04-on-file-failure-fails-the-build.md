**Changed** A per-file `on_file` callback that raises, or returns a row the
table rejects (a `strict` key mismatch, `NOT NULL`, `UNIQUE`, an
out-of-range integer), now fails the build and the query in every SDK, with
the error `` table `<table>`, file `<path>`: <original error> `` (the new
`DirSqlError::OnFile`). It used to skip a raising callback's file silently,
recorded only in `scan_failures()`, while a `NOT NULL` or `UNIQUE` rejection
failed the build with a bare SQLite error naming no file. A file re-read at
query time that fails now fails that query instead of dropping its rows.
`scan_failures()` is always empty.
