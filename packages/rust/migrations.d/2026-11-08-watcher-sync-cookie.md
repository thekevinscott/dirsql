### Rust: `Watcher::try_recv_all` returns a `Result` and waits for a sync cookie

#### Summary

`dirsql::watcher::Watcher::try_recv_all` now returns `Result<Vec<FileEvent>, notify::Error>`. On Linux it is a barrier: it returns every event the kernel queued before the call, rather than only those already copied off the kernel queue. Only direct users of the low-level `watcher` module are affected; `DirSQL`, the Python and TypeScript SDKs and the CLI need no change.

#### Required changes

| Before | After |
| --- | --- |
| `let events = watcher.try_recv_all();` | `let events = watcher.try_recv_all()?;` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- On Linux, each `Watcher` owns a `dirsql-cookies-*` directory under the OS temp dir, removed when the watcher drops.
- A watcher nobody drains stops buffering after 16384 events per channel; the backlog ends in `FileEvent::Rescan`.

#### Verification

```bash
cargo build
# expected: no error[E0308] at any `try_recv_all()` call site
```
