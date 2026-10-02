### Windows on-file arguments reach MSYS children unexpanded

**Summary**

On Windows, every argument of an `on-file` command is now passed double-quoted
by the MS C-runtime rules. An MSYS/Cygwin child (Git for Windows' `sh`,
`printf`, `cat`) previously glob- and brace-expanded any argument without a
space or tab; it now receives the argument as written. MSVCRT children parse
the same values as before. No signature changed, and Unix is unchanged.

**Required changes**

_None._

**Deprecations removed**

_None._

**Behavior changes without code changes**

| Surface (Windows, MSYS child) | Before | After |
|---|---|---|
| `on-file = "printf '[{}]'"` | child prints `[]`, zero rows | child prints `[{}]`, one row |
| an argument `*.txt` | expanded to matching files | the literal `*.txt` |

A config that relied on an MSYS child expanding a glob in its argv must ask
for a shell explicitly: `sh -c 'cat *.txt'`.

**Verification**

```toml
[[table]]
name = "t"
ddl = "CREATE TABLE t (n INTEGER)"
glob = "*.txt"
on-file = "printf '[{\"n\":1}]'"
```

```bash
dirsql query "SELECT n FROM t LIMIT 1" -c .dirsql.toml
# [{"n":1}]
```
