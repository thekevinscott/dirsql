### `scan_failures` is removed

#### Summary

Rust `DirSQL::scan_failures()` and the `OnFileFailure` struct are removed. The list was always empty: a per-file `on_file` callback that fails, or returns a row the table rejects, fails the build and the query with `DirSqlError::OnFile`. This breaks code that calls the method or names the type.

#### Required changes

| Before | After |
|---|---|
| `db.scan_failures()` | Delete the call; handle the `Err(DirSqlError::OnFile { table, path, message })` that `build()` and `query()` return |
| `use dirsql::OnFileFailure;` | Delete the import |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```sh
grep -rn 'scan_failures\|OnFileFailure' src/
cargo check
```

`grep` prints nothing and `cargo check` succeeds.
