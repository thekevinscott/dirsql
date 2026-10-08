### The watcher re-syncs after dropped file events

#### Summary

When the OS reports that file events were dropped, dirsql re-walks the
watched tree and emits the row events needed to match disk. Nothing
breaks. A re-sync that changes an `on-file` table reports its events with
`file_path` set to `.`, since no single file triggered them.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A watch stream may now carry a burst of events after a queue overflow.

#### Verification

```bash
dirsql --help
# expected: the usage text prints
```
