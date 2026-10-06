### Core: unbalanced braces and brackets in a glob are literal

#### Summary

A `{`, `}` or `[` with no partner in a glob is now a literal character, as in
bash, on path-tables and on config `[[table]] glob`. It used to fail with a
glob compile error. Nothing that compiled before changes meaning.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./{a.md'`, `'./a}.md'`, `'./[a.md'` and `'./a[b.md'` match files named
  exactly that instead of failing to compile.

#### Verification

```bash
cd "$(mktemp -d)"
touch '[a.md'
dirsql query "SELECT path FROM './[a.md'"
# expected: [{"path":"[a.md"}]
```
