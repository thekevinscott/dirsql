### Core: directory walks read sibling directories in parallel

#### Summary

Path-table scans and the startup scan read sibling directories on spare
cores. The files returned and their order are unchanged; nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p a b
touch a/2.md a/1.md b/1.md
dirsql query "SELECT path FROM './**/*.md'"
# expected: [{"path":"a/1.md"},{"path":"a/2.md"},{"path":"b/1.md"}]
```
