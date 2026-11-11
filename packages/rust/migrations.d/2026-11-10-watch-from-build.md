### The watcher starts at build; `start_watching()` is removed

#### Summary

Every handle with a named table now starts its file watcher before the build walks the directory and keeps it for its lifetime. `DirSQL::start_watching()` and `AsyncDirSQL::start_watching()` are removed; `watch()` and `poll_events()` attach to the running watcher.

#### Required changes

| Before | After |
| --- | --- |
| `db.start_watching()?;` | Delete the call; the watcher is already running. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A change made while the build walks reaches the built index or the first poll rather than being missed.

#### Verification

`cargo build` succeeds once every `start_watching()` call is deleted.
