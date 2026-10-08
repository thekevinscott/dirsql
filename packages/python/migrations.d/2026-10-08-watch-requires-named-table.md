### `watch()` requires a named table

#### Summary

`watch()`, `poll_events()`/`pollEvents()` and `start_watching()`/`startWatcher()` on an instance with no named tables now raise an error naming the cause, rather than watching and silently yielding nothing. Path-tables emit no watch events. Instances with at least one named table are unaffected.

#### Required changes

| Before | After |
| --- | --- |
| `DirSQL(root).watch()` yields nothing forever | Define a table (programmatic or `[[table]]`), or stop calling `watch()` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

A path-table-only instance that called `watch()` now gets an error. `dirsql server` with no tables returns 503 from `/events`.

#### Verification

Run `dirsql server -c <config with no [[table]]>` and `curl -i http://localhost:7117/events`; expect `503` naming "path-tables emit no watch events".
