### Revert read-on-close-write and the foreign-key offenders list

#### Summary

Two Rust-core changes from the previous release are reverted. Linux watchers read files on create and modify again, and `RawFileEvent::MovedIn` is removed. `ForeignKeyViolation` returns to its `table` and `file` fields and loses `offenders`. Rust code written against the previous release's additions breaks; the Python and TypeScript SDKs have no API change.

#### Required changes

| Before | After |
|---|---|
| `RawFileEvent::MovedIn(path) => ...` | remove the arm; a rename into the tree arrives as `RawFileEvent::Created(path)` |
| `violation.offenders` | `violation.table` and `violation.file` (the first offender) |

#### Deprecations removed

_None._

#### Behavior changes without code changes

On Linux, a watched file is read when it is created or modified, as before the previous release, so a half-written file can produce a parse error until it is complete. A foreign-key violation message with more than ten offending rows no longer names the files past the tenth row.

#### Verification

```bash
cargo build -p dirsql
# expected: builds; any code matching RawFileEvent::MovedIn or reading .offenders fails to compile until updated
```
