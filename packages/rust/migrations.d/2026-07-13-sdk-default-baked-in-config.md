### SDK no-config default is the baked-in `files` table; `from_config(root)` removed (#603)

#### Summary

Completes the "default config = the baked-in shipped config" model on the SDK
side (the CLI moved in #602). Two changes to the Rust construction surface:
(1) a builder with no `.config()` and no programmatic tables now injects the
baked-in default `files` table instead of producing an empty index; and (2) the
root-joining `DirSQL::from_config(root)` / `AsyncDirSQL::from_config(root)`
shortcuts — which implicitly read `<root>/.dirsql.toml` — are removed. The
explicit `from_config_path(path)` / `.config(path)` constructors are unchanged.
This is the shared core, so the Python and TypeScript SDKs get the new default
too (their signatures are unchanged; they never had a root-joiner).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Read `<root>/.dirsql.toml` via the shortcut | `DirSQL::from_config(root)` | `DirSQL::from_config_path(root.join(".dirsql.toml"))` or `DirSQL::builder().root(root).config(root.join(".dirsql.toml")).build()` |
| Async variant | `AsyncDirSQL::from_config(root)` | `DirSQL::builder().root(root).config(root.join(".dirsql.toml")).build_async()` (or `AsyncDirSQL::from_config_path(root.join(".dirsql.toml"))`) |
| Want a truly empty index (no tables) | `DirSQL::new(root, vec![])` (was tableless) | Provide at least one `Table`; a builder with no config and no tables now serves the default `files` table |

#### Deprecations removed

_None._ (`from_config` was removed directly, not via a prior deprecation.)

#### Behavior changes without code changes

- A builder / `DirSQL::new(root, vec![])` / `with_ignore(root, vec![], ...)`
  with no config and no programmatic tables now exposes the baked-in `files`
  table (one row per file over the root) rather than an empty index. Code that
  relied on "no tables → querying any table errors" will instead find a `files`
  table populated from the directory.

#### Verification

```rust
use dirsql::{DirSQL, Value};
// No config, no tables -> the baked-in `files` table is served:
let db = DirSQL::builder().root(".").build().unwrap();
let rows = db.query("SELECT basename FROM files LIMIT 1").unwrap(); // Ok, not "no such table"
assert!(matches!(rows.first().map(|r| &r["basename"]), Some(Value::Text(_))) || rows.is_empty());

// Explicit config still works; the removed shortcut's replacement:
// let db = DirSQL::from_config_path(std::path::Path::new(".").join(".dirsql.toml"))?;
```
