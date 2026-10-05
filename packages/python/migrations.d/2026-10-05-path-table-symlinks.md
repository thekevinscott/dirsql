### Core: path-tables follow symlinks as bash globstar does

#### Summary

Path-tables used to skip symlinks: a symlinked file was never a row and a
symlinked directory was never entered. They now follow bash with `globstar`
on. A symlinked file is a file. A symlinked directory is entered by any path
component except `**`. A broken link lists nothing. Every surface that
resolves a path-table is affected: the CLI, the REPL, the HTTP server, the
SDKs, and the `--on-file` form. No call changes; a query over a tree with
symlinks can return more rows. Declared `[[table]]` globs are unchanged.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./*'` and `'./**'` list symlinked files; `'./link.md'` lists the link.
- `'./*/*'` and `'./linkdir/**'` reach files through a symlinked directory.
- `'./**'` still never enters a symlinked directory, though `'./**/*.md'`
  lists `*.md` files directly inside one.

#### Verification

```python
# root holds top.md, real/r.md, link.md -> top.md and linkdir -> real
rows = await DirSQL(root).query("SELECT path FROM './*/*' ORDER BY path")
# expected: [{"path": "linkdir/r.md"}, {"path": "real/r.md"}]
rows = await DirSQL(root).query("SELECT path FROM './*' ORDER BY path")
# expected: [{"path": "link.md"}, {"path": "top.md"}]
```
