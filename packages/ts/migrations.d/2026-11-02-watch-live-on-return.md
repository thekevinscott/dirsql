### `watch()` starts the watcher before it returns

#### Summary

The TypeScript `watch()` now starts the native watcher before it returns; called before the initial scan finishes, it starts the watcher before `ready` resolves. A file written after both `watch()` and `ready` have returned produces an event and reaches the index. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

On an instance with no named tables, `watch()` called after `ready` throws immediately instead of on the stream's first `next()`. `startWatcher()` now registers the watcher on the JS thread instead of the libuv threadpool. A sleep between `watch()` and the first write is no longer needed.

#### Verification

`await db.ready; const stream = db.watch();`, write a matching file, then `await stream.next()`: expect an `insert` event, and `await db.query(...)` returns the row.
