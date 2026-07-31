**Changed** — a scan now attempts every matched file instead of stopping at the
first whose `on-file` hook fails, and reports every failure rather than only the
first. A single failure still raises `DirSqlError::OnFile` unchanged; several
now raise a new `DirSqlError::OnFileMany` carrying one `OnFileFailure { path,
message }` per file.

A scan with failures still fails and still leaves the cache exactly as it was —
only the reporting improved. Committing a partial index is tracked in #697.
