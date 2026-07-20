### Path-table globs are directory-aware and `*` no longer crosses directories (#628)

#### Summary

Path-table names were translated to globs by string surgery: `'./X'` became the
glob `X`, matched with separator-crossing semantics. Two consequences were
wrong. `'./docs'` matched only a *file* literally named `docs` (so a directory
returned nothing), and `'./*'` — the explicit non-recursive spelling — crossed
`/` and behaved identically to `'./**/*'`. Both are fixed: a directory is
recursive by default and `*` matches within one directory. Path-table scans
also now apply skip rules, and the three previously-unsupported prefixes
(`/`, `../`, `~/`) resolve. No API signature changes; the affected surface is
the set of rows a path-table returns.

#### Required changes

| You wrote | Before | Now |
| --- | --- | --- |
| `'./*'` | every file at any depth | files directly in the index root |
| `'./docs/*'` | every file under `docs/` at any depth | files directly in `docs/` |
| `'./docs'` | no rows (matched a *file* named `docs`) | every file under `docs/`, recursively |
| `'./'` | every file, `node_modules`/`.git` included | the same, minus the skip rules |
| `'/var/log/*.log'` | error: not supported yet | resolves; absolute `path` values |

To recover the old recursive meaning of `'./*'`, write `'./**/*'`. To see files
the skip rules now hide, name the directory outright
(`'./node_modules/*/package.json'`).

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./'` and other recursive path-tables no longer return files under
  `node_modules/` or `.git/`, nor files matching the configured `ignore`
  patterns. Skip rules are judged below the literal part of the written path,
  so explicitly naming a skipped directory still scans it.
- `/`, `../` and `~/` path-tables stop erroring and start returning rows. Their
  `path` column is **absolute**, unlike a `./` table's index-root-relative
  `path`.

#### Verification

In a directory containing `docs/a.md`, `docs/nested/deep.md`, `top.md` and a
`node_modules/`:

```console
$ dirsql query "SELECT path FROM './*'"
[{"path":"top.md"}]

$ dirsql query "SELECT path FROM './docs'"
[{"path":"docs/a.md"},{"path":"docs/nested/deep.md"}]

$ dirsql query "SELECT path FROM './'" | grep -c node_modules
0
```
