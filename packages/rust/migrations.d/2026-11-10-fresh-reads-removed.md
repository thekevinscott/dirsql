### `set_fresh_reads` and `read_changes` are removed

#### Summary

The hidden (`#[doc(hidden)]`) Rust methods `DirSQL::set_fresh_reads` and `DirSQL::read_changes` are removed, because queries no longer walk each table's glob. A caller of either no longer compiles. Python and TypeScript never exposed them.

#### Required changes

| Before | After |
| --- | --- |
| `db.set_fresh_reads(false);` | Delete the call. |
| `db.read_changes()?;` | Delete the call. |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

`cargo build` succeeds once every `set_fresh_reads` and `read_changes` call is deleted.
