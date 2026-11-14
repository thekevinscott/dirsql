### `watch()` attaches to the running watcher

#### Summary

`watch()` attaches to the file watcher the instance started when it was built, instead of starting it. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On an instance with no named tables, `db.watch()` called after `await db.ready()` returns a stream, and the error naming the cause is raised from its first iteration.

#### Verification

`async for _ in db.watch(): break` on a `DirSQL` with no tables raises an error containing `path-tables emit no watch events`.
