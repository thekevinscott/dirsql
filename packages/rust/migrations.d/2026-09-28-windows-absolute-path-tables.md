### Windows absolute path-tables resolve and report `/` paths

**Summary**

On Windows, a path-table named by a drive letter (`C:\logs\*.log`), a UNC share
or `~\` now resolves; it previously failed with `no such table`. Absolute
path-tables (`/`, `../`, `~/` and the Windows forms) report `path` with `/`
separators on Windows. No signature changed, and Unix output is unchanged.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface (Windows) | Before | After |
|---|---|---|
| `SELECT path FROM 'C:\logs\*.log'` | `no such table` | `C:/logs/app.log` |
| `SELECT path FROM '~/notes/*.md'` | `C:\Users\u\notes\n.md` | `C:/Users/u/notes/n.md` |

A Windows consumer that matched on `\` in an absolute path-table's `path` must
match on `/`.

**Verification**

```bash
dirsql "SELECT path FROM 'C:/Windows/*.ini' LIMIT 1"
# [{"path":"C:/Windows/system.ini"}]
```
