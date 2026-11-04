### Text that is not valid UTF-8 fails the query

#### Summary

A query returning a text value that is not valid UTF-8 now fails with `column "<name>" holds text that is not valid UTF-8` (the new `DbError::NotUtf8`) in every CLI format and SDK, instead of crashing. `--format json`, which printed such text with replacement characters, now fails the same way. Queries over valid text are unchanged.

#### Required changes

To keep reading such a value, select its bytes instead of its text:

| Before | After |
|---|---|
| `SELECT v FROM t` | `SELECT hex(v) AS v FROM t` or `SELECT CAST(v AS BLOB) AS v FROM t` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

`dirsql "SELECT CAST(x'ff41' AS TEXT) AS t" --format json` printed `[{"t":"�A"}]` (a replacement character) and exited 0; it now exits non-zero with the error above. A Rust caller matching on `DbError` exhaustively needs an arm for `NotUtf8`.

#### Verification

`dirsql "SELECT CAST(x'ff' AS TEXT) AS t" --format json` exits non-zero and prints `column "t" holds text that is not valid UTF-8` on stderr.
