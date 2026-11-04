### `.//` path-tables resolve under the index root

#### Summary

A path-table whose leading `./` is followed by more separators (`'.//docs/a.md'`) now resolves under the index root instead of the filesystem root. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

`'.//x'` returns the rows `'./x'` returns. It no longer reads `/x`.

#### Verification

```bash
dirsql "SELECT path FROM './/docs/*.md'"
```

Prints the same rows as `dirsql "SELECT path FROM './docs/*.md'"`.
