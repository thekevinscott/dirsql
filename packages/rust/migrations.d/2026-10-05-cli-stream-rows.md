### CLI: `dirsql query` streams its JSON rows

#### Summary

`dirsql query` builds less in memory before printing JSON. The bytes printed
are unchanged; nothing breaks.

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
dirsql query "SELECT path, path FROM './*'"
# expected: [{"path":"a.md"},{"path":"b.md"}]
```
