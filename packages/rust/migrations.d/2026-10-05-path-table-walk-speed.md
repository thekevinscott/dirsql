### Core: faster path-table walk on large directories

#### Summary

Path-table scans of large directories are faster. The files returned and
their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
touch b.md a.md
dirsql query "SELECT path FROM './*'"
# expected: [{"path":"a.md"},{"path":"b.md"}]
```
