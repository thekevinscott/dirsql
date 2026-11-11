### `watch()` attaches to the running watcher

#### Summary

`watch()` attaches to the file watcher the instance started when it was built, instead of starting it. `startWatcher()` is no longer needed before `pollEvents()`; it remains and only attaches. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On an instance with no named tables, `db.watch()` called after `await db.ready` returns a stream, and the error naming the cause rejects its first `next()`.

#### Verification

`await db.watch().next()` on a `DirSQL` with no tables rejects with an error matching `/path-tables emit no watch events/`.
