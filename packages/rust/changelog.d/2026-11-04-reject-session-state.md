**Fixed** `query` (CLI, REPL, `POST /query`, every SDK) rejects `BEGIN`,
`COMMIT`, `ROLLBACK`, `SAVEPOINT`, `RELEASE` and any `PRAGMA` that sets a value
as not authorized. SQLite classifies them as read-only, so they used to run on
the shared index connection and outlive the statement: an open transaction
hid the watcher's next update for good, and a setter such as
`PRAGMA case_sensitive_like=1` changed what every later statement returned,
across clients on the server. Reading pragmas (`PRAGMA table_info(t)`,
`PRAGMA cache_size`, `PRAGMA integrity_check(1)`) still work.
