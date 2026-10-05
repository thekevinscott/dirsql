### Core: `?` and `[...]` in globs match a character, not a byte

#### Summary

`?` and bracket expressions in path-tables and config `[[table]] glob`
matched one byte, so they missed names with non-ASCII characters. They now
match one character, as bash does in a UTF-8 locale. No call changes; a glob
over non-ASCII names can return different rows. ASCII names are unaffected.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./caf?.md'` lists `café.md`; `'./na[ï]ve.txt'` lists `naïve.txt`.
- `'./na??ve.txt'` no longer lists `naïve.txt`.

#### Verification

```bash
cd "$(mktemp -d)"
touch café.md naïve.txt
dirsql query "SELECT path FROM './caf?.md'"
# expected: [{"path":"café.md"}]
dirsql query "SELECT path FROM './na??ve.txt'"
# expected: []
```
