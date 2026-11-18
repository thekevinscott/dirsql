### Rust core: hidden `.no_watch(bool)` builder option

**Summary**

The `#[doc(hidden)]` builder method `.no_watch(true)` builds from one walk and starts no watcher; `watch()` / `poll_events()` then return `DirSqlError::Watch`. Additive: the option defaults to watching, so nothing breaks.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

_None._

**Verification**

```bash
cargo test -p dirsql --lib no_watch
# test result: ok.
```
