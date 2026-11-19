### `Db::promote()` and `Db::reader()`

#### Summary

`Db::promote()` moves the default anonymous index database into a temp file, `dirsql-<pid>-<n>.db`, in WAL mode, and `Db::reader()` opens another connection to it that reads while the writer writes. Nothing calls `promote()` yet. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ A promoted database's `dirsql-<pid>-<n>.db` and its `-journal`, `-wal` or `-shm` files sit in the temp directory (`SQLITE_TMPDIR`, then `TMPDIR`, then `/var/tmp`, `/usr/tmp`, `/tmp`) until its last connection closes; a process killed before that (for example by `SIGKILL`) leaves them there.

#### Verification

Call `Db::reader()` on a `Db::new()` database: it returns `DbError::Anonymous`. After `db.promote()`, it returns a reader that sees the writer's commits.
