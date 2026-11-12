### Queries raise while an orphan stands

#### Summary

A query raises the foreign-key violation while a change on disk leaves a child row with no parent, and succeeds again once the parent appears or the child is removed. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A query that returned the last consistent rows while an orphan stood now raises the foreign-key violation instead, on every call until a later change resolves it, as a failing `on_file` hook raises on every query until its file is fixed. When both stand, the query raises the failing file's error first (the first by table, then path); the foreign-key violation is raised once no file is failing. `watch()` consumers see the same error events as before.

#### Verification

On Linux, open a `DirSQL` over two tables linked by a foreign key, write a child file naming a missing parent, and query: the query raises `foreign key violation`. Write the parent file and query again: both rows are returned.
