### `..` in absolute and `~/` path-tables resolves physically

#### Summary

An absolute or `~/` path-table containing `..` now lets the filesystem resolve it, so a `..` after a symlinked directory climbs out of the link's target, as in the shell. The `path` column keeps the `..` as written. No API changes; nothing breaks for paths without a symlink ahead of a `..`, except that their `path` now shows the `..` as written.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

| Before | After |
|---|---|
| `SELECT path FROM '/a/b/../*.md'` reports `/a/x.md` | reports `/a/b/../x.md` |
| `'/a/link/../f.md'` with `link -> /real/sub` reads `/a/f.md` | reads `/real/f.md`, as `cat` does |

Windows paths keep folding `..` lexically, as Windows itself does.

#### Verification

```sh
mkdir -p r/real/sub r/y && printf P > r/real/f.md && printf L > r/y/f.md && ln -s "$PWD/r/real/sub" r/y/link
dirsql "SELECT content FROM '$PWD/r/y/link/../f.md'"
```

Expected: `content` is `P`.
