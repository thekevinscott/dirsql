### Watch: Linux reads a file when it is closed

#### Summary

On Linux the watcher reads a file on close-after-write or rename-into-place
instead of on create or write. Affects `watch()`, `poll_events()` and the CLI
`--watch` stream on Linux only; macOS and Windows are unchanged. Separately,
on every platform a rename into the tree is now reported as
`RawFileEvent::MovedIn` instead of `RawFileEvent::Created`, which breaks an
exhaustive `match` on `RawFileEvent`.

#### Required changes

- Handle `RawFileEvent::MovedIn(path)` wherever you `match` on
  `RawFileEvent`. Treat it as you treated `Created`.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A file is indexed when its writer closes it. While it is held open its
  previous rows stay and no event is emitted for it.
- A `chmod` or `touch` on a file no longer re-reads it.
- Scanning a newly created directory emits no error for a file that fails
  to parse; the row appears once that file is closed. A directory renamed
  into the tree still reports such a file as an error.

#### Verification

On Linux, with a `DirSQL` built over a directory with a `*.json` table whose
`on_file` parses JSON, and `start_watching()` called:

```rust
use std::io::Write;
let mut f = std::fs::File::create(root.join("a.json"))?;
f.write_all(br#"{"name": "#)?;
assert!(db.poll_events(Duration::from_secs(1))?.is_empty()); // nothing yet
f.write_all(br#""a"}"#)?;
drop(f);
// expected: the next poll_events returns one RowEvent::Insert
```
