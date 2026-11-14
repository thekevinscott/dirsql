### Watch failures are fatal

#### Summary

A watcher that cannot see every change now fails the build or every later query instead of leaving the index silently stale. No API changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

When inotify's watch limit is exhausted, the build fails with an `std::io::Error` (`DirSqlError::Io`, kind `StorageFull`) whose `DirSqlError::os_error()` is errno 28, `ENOSPC` and the message `inotify watch limit reached`; to handle it, match `DirSqlError::os_error()` for errno 28. Any other error starting the watcher also fails the build, with `DirSqlError::Watch`; a table directory that does not exist yet still does not. After the build, if a new directory cannot be watched or the OS drops file events, every later query fails until you build a new `DirSQL`.

#### Verification

On Linux as a non-root user, build `DirSQL` over a directory, create a subdirectory with mode `000`, and run `db.query(...)` twice: both fail with `Permission denied`.
