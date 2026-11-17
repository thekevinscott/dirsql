### Stored text that is not valid UTF-8 fails a table refresh instead of crashing

#### Summary

Re-reading a declared table's stored rows after a file changes, which the watcher does on every event, panicked when a cell held text that is not valid UTF-8. It now fails that refresh with `column "<name>" holds text that is not valid UTF-8` (`DbError::NotUtf8`), as queries already did. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

_None._
