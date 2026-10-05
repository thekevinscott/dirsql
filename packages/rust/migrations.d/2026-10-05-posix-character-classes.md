### Core: POSIX character classes match in globs

#### Summary

A bracket holding a POSIX character class, such as `'./[[:digit:]].md'`,
used to match nothing. It now matches as bash does, on path-tables, declared
`[[table]]` globs and `ignore` patterns. Nothing breaks; such a pattern can
now return rows.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./[[:digit:]].md'` lists `1.md` and `5.md`; `'./[![:digit:]].md'` lists
  `a.md`.
- An unknown class name, such as `[[:foo:]]`, contributes no characters.

#### Verification

```bash
cd "$(mktemp -d)"
touch 1.md 5.md a.md
dirsql query "SELECT path FROM './[[:digit:]].md' ORDER BY path"
# expected: [{"path":"1.md"},{"path":"5.md"}]
```
