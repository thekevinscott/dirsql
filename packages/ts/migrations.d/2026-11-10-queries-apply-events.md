### Queries read the index; the watcher updates it

#### Summary

A query on a long-lived `DirSQL` reads the index as it stands. The watcher's events are applied in the background as they arrive, rather than by queries or by walking each named table's glob. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A write reaches the index shortly after it returns: a query issued the moment a write returns may read the previous rows. Wait for the change on `watch()` before querying. `watch()` and `pollEvents()` report the changes applied after they attach; a change applied before reaches neither. `onFile` runs on the JS main thread, so a change reaches the index when that thread is next free; a JS thread kept busy delays it. A file is read when it is closed after writing or renamed into place, not while a handle to it is still open. A file whose `onFile` fails after the build fails every query with that error until the file is fixed or removed; the index keeps the file's last rows meanwhile, and `watch()` consumers still receive the error. A query no longer raises the error of a file that breaks a foreign key; that error reaches `watch()` consumers. An edit to a `.gitignore` after the build is not picked up, nor is a symlink pointed at a new target or an edit to the file a symlink points at; build a new `DirSQL` to read them.

#### Verification

On Linux, build a `DirSQL` over a directory with two tables linked by a foreign key, write a parent file then its child in a loop from another thread, and query in a loop; every query returns while the writer runs, and none raises.
