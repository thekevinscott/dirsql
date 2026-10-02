### Hooks receive non-verbatim paths on Windows

**Summary**

On Windows, the `{path}` and `{root}` values substituted into `on-file` and
`parsed()` commands could carry the verbatim prefix (`\\?\C:\...`,
`\\?\UNC\server\share\...`). They are now the plain form. No signature
changed. Output on Unix is unchanged.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface (Windows only) | Before | After |
|---|---|---|
| `{path}` / `{root}` on a drive | `\\?\C:\data\a.txt` | `C:\data\a.txt` |
| `{path}` / `{root}` on a share | `\\?\UNC\server\share\a.txt` | `\\server\share\a.txt` |

A verbatim path whose plain spelling would name a different file (a reserved
device name, a trailing dot or space, a path of 260 or more UTF-16 units, or a
`\\?\GLOBALROOT` / `\\?\Volume{...}` form) is still passed verbatim.

**Verification**

```toml
[[table]]
name = "seen"
ddl = "CREATE TABLE seen (n INTEGER)"
glob = "*.txt"
on-file = "sh -c 'printf \"%s\\n\" \"$1\" >> seen.log; echo []' sh {path}"
```

```bash
dirsql "SELECT count(*) FROM seen"
type seen.log
# each line starts with C:\, not \\?\
```
