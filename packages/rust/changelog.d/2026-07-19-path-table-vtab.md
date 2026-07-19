**Added**

Internal glob-backed virtual table (`dirsql::vtab`) serving filesystem rows: the seven stat columns (`path`, `basename`, `dir`, `ext`, `size`, `mtime`, `ctime`) plus a hidden `content` column read from disk only when a query names it. Registered as the `dirsql_path` SQLite module and read-only — writes are rejected by SQLite itself. The scan runs per statement, so reads are live.

Also adds `scanner::scan_glob`, the single-glob counterpart to `scan_directory`, sharing the same walker and the reserved `.dirsql/` skip rule.

Not yet reachable from user SQL; the query-path fallback that exposes it lands separately.
