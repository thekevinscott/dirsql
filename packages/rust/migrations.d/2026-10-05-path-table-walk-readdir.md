### Core: path-table walk reads directory names into one buffer

#### Summary

Path-table scans read directory names with fewer allocations. The files
returned and their order are unchanged; nothing breaks.

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
