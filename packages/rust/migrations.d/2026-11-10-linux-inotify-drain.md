### Watch: Linux reads inotify directly, with a drain call

#### Summary

On Linux the watcher reads inotify through the `inotify` crate instead of
notify, reports a file on close-after-write or rename-into-place, and gains
`Watcher::drain`. Affects `watch()`, `poll_events()` and the CLI watch stream
on Linux only. `RawFileEvent` is unchanged and nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- On Linux a file held open by its writer emits no event until it is closed.
- On Linux a `chmod` or `touch` on a file no longer re-reads it.
- On Linux a finished write is reported as `RawFileEvent::Created`, never
  `RawFileEvent::Modified`.

#### Verification

On Linux:

```rust
let watcher = dirsql::watcher::Watcher::new(&root)?;
std::fs::write(root.join("a.txt"), "x")?;
// expected: [Created(".../a.txt")]
println!("{:?}", watcher.drain());
```
