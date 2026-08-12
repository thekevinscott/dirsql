### Persisted path-tables reuse a parser's output across runs

#### Summary

Under `--persist`, a path-table parsed with `--on-file` now serves an unchanged
file's rows from the cache instead of re-running the parser (dirsql#825). No
API changed and no call site moves; what changes is how often your parser
command runs. A parser whose output is a function of the file it is handed —
the documented `on-file` contract, and what declared `[[table]]` hooks have
always assumed under `--persist` — sees no difference beyond speed. A parser
that returns something different for the same unchanged file (it reads the
clock, a network service, or an environment variable) will now be observed
returning the *previous* run's answer until the file itself changes.

#### Required changes

_None._ Nothing to update: the fix restores the behavior
`docs/howto/persist.md` already documented.

| Surface | Before | After |
| ------- | ------ | ----- |
| `dirsql query "… FROM './**/*.json'" --on-file '<cmd>' --persist` | the parser ran once per file on every run | the parser runs only for files whose stat metadata moved |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **Parsed path-tables under `--persist`**: previously the parser ran once per
  matched file on every run; now a file whose size/mtime/ctime/inode/device
  tuple matches the cache serves its cached rows and the process is not
  spawned. Cached rows are keyed by the scan root, the glob, the parser command
  and the `dirsql` version together, so changing any of them re-parses. A
  parser that is not a pure function of its input file should either be run
  without `--persist` or made to vary with the file.
- **The cache file on an unchanged run**: previously every startup rewrote the
  `_dirsql_meta` block, so `cache.db` changed even when nothing about the tree
  had; now a run that changes nothing writes nothing and the file is left
  byte-for-byte identical. Tooling that watched the cache's mtime as a
  "dirsql ran" signal needs a different signal.
- **The internal `dirsql_parsed` vtab module** takes a fifth fixed argument
  (the cache path, empty for none) between the gitignore switch and the ignore
  patterns. It is `#[doc(hidden)]` plumbing that `dirsql` mints for itself; a
  hand-written `CREATE VIRTUAL TABLE … USING dirsql_parsed(…)` must add the
  argument.

#### Verification

```bash
mkdir -p /tmp/dirsql-825 && cd /tmp/dirsql-825
printf '[{"id":1}]' > a.json
dirsql query "SELECT id FROM './*.json'" --on-file 'cat {path}' --persist
md5sum .dirsql/cache.db
time dirsql query "SELECT id FROM './*.json'" --on-file 'cat {path}' --persist
md5sum .dirsql/cache.db
# expected: the same rows, the second run near-instant, the same md5 both times
```
