**Added**

`Db::promote()` copies the default anonymous index database into a WAL-mode file of its own in SQLite's temp directory (`SQLITE_TMPDIR`, then `TMPDIR`, then `/var/tmp`, `/usr/tmp`, `/tmp`), deleted with its journal, `-wal` and `-shm` files once its last connection closes. `Db::reader()` opens another connection to a promoted or opened database, with the same path-table settings, extensions and declared functions, that reads while the writer writes. Nothing calls `promote()` yet, so the default database is still anonymous.
