### Queries apply watcher events

#### Summary

A query on a long-lived `DirSQL` applies the watcher's queued events rather than walking each named table's glob. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A file is read when it is closed after writing or renamed into place, not while a handle to it is still open. On macOS and Windows a query issued the moment a write returns may read the previous rows; wait for the change on `watch()` before querying. A query no longer raises the error of a file whose `on_file` fails or that breaks a foreign key; that error reaches `watch()` consumers.

#### Verification

On Linux, build a `DirSQL` over a directory with two tables linked by a foreign key, write a parent file then its child in a loop from another thread, and query in a loop; no query raises.
