### Core: a literal brace spelling names a gitignored file

#### Summary

A path glob that bash reads as a literal name, but whose name holds literal
braces (`./\{q\}.md`, `./{q}.md`), now gets the same `.gitignore` exemption as
any other wildcard-free path. Affects every path-table surface. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Such a glob now returns the named file even when `.gitignore` ignores it.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir .git
printf '*.md\n' > .gitignore
touch '{q}.md'
dirsql "SELECT path FROM './\{q\}.md'"
# expected: [{"path":"{q}.md"}]
```
