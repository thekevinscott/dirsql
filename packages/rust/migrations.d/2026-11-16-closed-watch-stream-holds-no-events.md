### `watch()` can be called again after its stream is dropped

#### Summary

`DirSQL::watch()` returns a new stream once the previous one has been dropped. A call while a stream is alive still returns `WatchAlreadyStarted`. No existing code has to change.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A second `watch()` after the first stream was dropped succeeds where it returned `WatchAlreadyStarted`. The new stream sees only the changes made after that call.

#### Verification

_None._
