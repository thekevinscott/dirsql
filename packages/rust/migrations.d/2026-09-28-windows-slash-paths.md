### Root-relative paths are `/`-separated on Windows

**Summary**

On Windows, dirsql reported root-relative paths with `\`: `file_path` in row
events, the `path` column of path-tables, and the rows the watcher wrote.
They now use `/` on every platform, matching Linux and macOS. No signature
changed. Output on Unix is unchanged.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface (Windows only) | Before | After |
|---|---|---|
| `SELECT path FROM './'` | `docs\a.md` | `docs/a.md` |
| `RowEvent` `file_path` | `nested\a.txt` | `nested/a.txt` |
| Path-table under an absolute prefix | `C:\data\logs\a.log` | `C:\data\logs/a.log` |

A persisted parsed-path-table cache written on Windows before this change is
keyed by `\` paths, so its first run after the upgrade re-parses every file.

**Verification**

```bash
dirsql "SELECT path FROM './' LIMIT 1"
# [{"path":"docs/a.md"}]  -- `/` separators on Windows too
```
