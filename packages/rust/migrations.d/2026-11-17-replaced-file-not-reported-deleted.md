### A watched file replaced by a rename is no longer reported deleted

#### Summary

On Windows, saving a watched file by renaming a staged copy over it sometimes produced a `delete` event for each of its rows followed by an `insert` for each, instead of an `update` for each changed row. A file the watcher reports removed is now re-read when it is still there. Nothing breaks and no code changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A removal the watcher reports for a file that exists when the report is applied yields the events of a re-read (`update`, `insert`, `delete` for the rows that differ) rather than a `delete` for every row. This also covers a file removed and written again before the removal is applied.

#### Verification

On Windows, watch a table over one file, rename a changed copy over that file in a loop, and read the events: every iteration reports only `update` events.
