### Content over SQLite's length limit reads as NULL

#### Summary

A path-table's `content` for a file longer than SQLite's string length limit (1e9 bytes by default) is now `NULL` instead of an error that failed the whole query. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`SELECT content FROM './**'` over a tree holding a file over 1e9 bytes used to exit non-zero with `string or blob too big`; it now returns every row, with that file's `content` `NULL`.

#### Verification

In an empty directory, `truncate -s 1000000001 big.txt` then `dirsql "SELECT basename, typeof(content) AS t FROM './*'" --format json` prints `[{"basename":"big.txt","t":"null"}]` and exits 0.
