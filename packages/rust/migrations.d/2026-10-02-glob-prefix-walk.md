### `scan_glob` loses its `ignore_base` argument

**Summary**

A `./` path-table is now resolved like an absolute one: the literal directory
chain ahead of the first wildcard becomes the scan root, and the glob is the
remainder. The walker therefore no longer needs to know which part of a glob
was named outright, so `dirsql::scanner::scan_glob` drops its `ignore_base`
parameter and `dirsql::path_table::ignore_base` is removed. Only Rust callers
of those two functions are affected; the CLI, the Python and TypeScript SDKs,
and every query are unchanged in what they return.

**Required changes**

| Surface | Before | After |
|---|---|---|
| `scanner::scan_glob` | `scan_glob(&root, &glob, &ignore, &ignore_base, gitignore)` | `scan_glob(&root, &glob, &ignore, gitignore)` |
| `path_table::ignore_base(glob)` | returned the glob's leading literal directories | removed; `path_table::resolve` now puts them in `PathTable::root` and `PathTable::path_prefix` |

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface | Before | After |
|---|---|---|
| `'./small/*.md'` beside a large sibling tree | walked the whole index root | walks `small/` only |
| `'./.dirsql'` | zero rows | scans the reserved directory, as naming it outright does for every other skipped directory |
| `'./x/.dirsql/**'` | scanned | pruned, as the reserved directory is at any scan root |
| a symlinked directory named ahead of the first wildcard | never entered | entered, as for an absolute table |

**Verification**

```bash
mkdir -p /tmp/t/small /tmp/t/big
touch /tmp/t/small/a.md /tmp/t/big/b.md
cd /tmp/t
dirsql "SELECT path FROM './small/*.md'"
# [{"path":"small/a.md"}]
```
