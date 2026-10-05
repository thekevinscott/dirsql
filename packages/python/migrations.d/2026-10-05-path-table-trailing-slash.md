### Core: a trailing `/` on a path-table is `*` appended

#### Summary

A path-table written with a trailing `/` now means that path with `*`
appended. `'./*/'` lists the files directly inside each top-level directory,
the same rows as `'./*/*'`, where it used to return the top-level files of
`'./*'`. The same holds for absolute, `../` and `~/` path-tables on every
surface that resolves one: the CLI, the REPL, the HTTP server and the SDKs.
`'./'`, `'./docs/'` and `'./**/'` are unchanged.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Top-level files | `SELECT * FROM './*/'` | `SELECT * FROM './*'` or `SELECT * FROM './'` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./*/'`, `'./docs/*/'`, `'/var/*/'`, `'../*/'` and `'~/*/'` list the files
  directly inside each directory the glob matches, not the matches themselves.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p docs
touch top.md docs/a.md
dirsql query "SELECT path FROM './*/' ORDER BY path"
# expected: [{"path":"docs/a.md"}]
```
