### Core: path tables stat only when a stat column is read

#### Summary

Path-table queries skip the per-file stat unless the statement reads `size`,
`mtime`, `ctime` or `content`. Results are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
printf hello > a.md
dirsql query "SELECT path, size FROM './*'"
# expected: [{"path":"a.md","size":5}]
```
