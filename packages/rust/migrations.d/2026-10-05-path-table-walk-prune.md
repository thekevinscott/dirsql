### Core: path-tables stop descending where their glob cannot match

#### Summary

A path-table whose glob has no `**` used to read every directory below where
its scan started, then discard what the glob could not match. It now enters a
directory only while some component of the glob can still match inside it.
The rows returned are identical; only the cost changes. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p docs/deep
touch docs/a.md docs/deep/b.md
dirsql query "SELECT path FROM './docs/*.md'"
# expected: [{"path":"docs/a.md"}]
```
