**Removed** `DirSQL::scan_failures()` and the `OnFileFailure` struct. The list
has been empty in every case since a failing per-file `on_file` callback began
failing the build and the query with `DirSqlError::OnFile`.
