### Pre-1970 `mtime` and `ctime` are negative seconds

#### Summary

A path-table's `mtime` and `ctime` for a time before 1970 are now negative Unix seconds instead of NULL, in the CLI and every SDK. Times from 1970 on are unchanged. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`mtime IS NULL` no longer matches a file modified before 1970; `mtime < 0` does.

#### Verification

`touch -d '@-86400' neg.md` then `dirsql "SELECT mtime FROM './neg.md'" --format json` prints `[{"mtime":-86400}]`.
