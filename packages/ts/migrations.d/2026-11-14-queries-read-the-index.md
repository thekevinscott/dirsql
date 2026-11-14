### Queries read the index; the watcher updates it in the background

#### Summary

A query on a long-lived `DirSQL` reads the index as it stands and no longer applies the watcher's queued events first. The events are applied in the background as they arrive. No API or configuration changes are required.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

A write reaches the index shortly after it returns. On Linux a query issued the moment a write returns may now read the previous rows, as it already could on macOS and Windows; wait for the change on `watch()` before querying. `watch()` and `pollEvents()` report the changes applied after they attach; a change applied before then reaches neither. `onFile` runs on the JS main thread, so a change reaches the index when that thread is next free; a JS thread kept busy delays it. A query never waits on the watcher, so a writer that never pauses no longer holds it.

#### Verification

Build a `DirSQL` over a directory, rewrite a file under a table's glob in a loop from another thread, and query in a loop; every query returns while the writer runs.
