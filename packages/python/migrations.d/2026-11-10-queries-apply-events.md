### Queries apply watcher events

#### Summary

A query on a long-lived `DirSQL` applies the watcher's queued events rather than walking each named table's glob. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A file is read when it is closed after writing or renamed into place, not while a handle to it is still open. On macOS and Windows a query issued the moment a write returns may read the previous rows; wait for the change on `watch()` before querying. A file whose `on_file` fails after the build fails every query with that error until the file is fixed or removed; the index keeps the file's last rows meanwhile, and `watch()` consumers still receive the error. A query no longer raises the error of a file that breaks a foreign key; that error reaches `watch()` consumers. An edit to a `.gitignore` after the build is not picked up, nor is a symlink pointed at a new target or an edit to the file a symlink points at; build a new `DirSQL` to read them.

#### Verification

On Linux, build a `DirSQL` over a directory with two tables linked by a foreign key, write a parent file then its child in a loop from another thread, and query in a loop; no query raises.
