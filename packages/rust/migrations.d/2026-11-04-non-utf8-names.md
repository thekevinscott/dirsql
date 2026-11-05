### Path-table columns keep names that are not valid Unicode

#### Summary

A path-table's `path`, `basename`, `dir`, and `ext` hold a file name's exact bytes, so a name that is not valid Unicode no longer turns into a replacement character. Selecting such a column as text now fails the query, where it used to print the replacement-character path. An `on-file` command receives the name as it is on disk. Names that are valid Unicode are unchanged.

#### Required changes

To read a column that may hold such a name, select its bytes:

| Before | After |
|---|---|
| `SELECT path FROM './*'` | `SELECT hex(path) AS path FROM './*'` or `SELECT CAST(path AS BLOB) AS path FROM './*'` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

On a filesystem that allows such names, `dirsql "SELECT path FROM './*'"` printed `ok�name.txt` for a file named `ok\xffname.txt`, and exited 0; it now exits non-zero with `column "path" holds text that is not valid UTF-8`. An `on-file` command that received `/dir/ok�name.txt` receives `/dir/ok\xffname.txt`.

#### Verification

On tmpfs, `printf A > "$(printf 'ok\377name.txt')"`, then `dirsql "SELECT hex(path) AS p FROM './*'" --format json` prints `[{"p":"6F6BFF6E616D652E747874"}]`.
