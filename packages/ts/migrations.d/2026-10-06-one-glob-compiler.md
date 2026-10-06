### Config table globs share the path-table glob compiler

#### Summary

A declared `[[table]] glob` is now compiled by the same compiler as a
path-table. A brace range such as `f{1..3}.md` expands as in bash instead of
matching nothing. Nothing else changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `glob = "f{1..3}.md"` matches `f1.md`, `f2.md` and `f3.md`.

#### Verification

```bash
cd "$(mktemp -d)"
touch f1.md f2.md f3.md f4.md
dirsql query "SELECT path FROM './f{1..3}.md' ORDER BY path"
# expected: [{"path":"f1.md"},{"path":"f2.md"},{"path":"f3.md"}]
```
