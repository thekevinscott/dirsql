### A new folder's walk is checked with the changes that arrived during it

#### Summary

The walk of a folder created while the index is open now applies the changes the watcher queued during that walk before foreign keys are checked, as startup does. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A change that arrives while a new folder is walked can be delivered in the same batch as that folder's rows, rather than in a later one. A folder among those changes is walked in the same batch too, so while folders are created faster than they are walked, queries wait until the walking catches up.

#### Verification

_None._
