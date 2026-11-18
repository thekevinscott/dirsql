### The default index database is a temp file

#### Summary

The default (non-persisted) index now lives in a named file, `dirsql-<pid>-<n>.db`, in SQLite's temp directory instead of an anonymous temp database, and is deleted on shutdown. It switches to WAL mode once a second connection opens it with `Db::reader()`. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

While dirsql runs, `dirsql-<pid>-<n>.db` and its `-journal`, `-wal` or `-shm` files exist in the temp directory (`SQLITE_TMPDIR`, then `TMPDIR`, then `/var/tmp`, `/usr/tmp`, `/tmp`). A process killed before shutdown (for example by `SIGKILL`) leaves them there.

#### Verification

`ls "${SQLITE_TMPDIR:-${TMPDIR:-/var/tmp}}"/dirsql-*.db` while a dirsql server runs lists its database; after it exits cleanly, the files are gone.
