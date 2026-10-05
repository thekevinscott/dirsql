### Core: a path-table directory name is one level

#### Summary

A path-table that names a directory -- `'./'`, `'./docs'`, `'./docs/'`, an
absolute, `../` or `~/` directory -- now lists only the files directly
inside it, like `ls`. It used to expand to `<dir>/**/*` and scan every
depth. `*` matches one level and `**` any depth, as in the shell. Every
surface that resolves a path-table is affected: the CLI, the REPL, the HTTP
server, the SDKs, and the `--on-file` form. A query that relied on a
directory name descending must spell the recursion with `**`.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Whole tree | `SELECT * FROM './'` | `SELECT * FROM './**'` |
| Whole subtree | `SELECT * FROM './docs'` | `SELECT * FROM './docs/**'` |
| Absolute subtree | `SELECT * FROM '/var/log'` | `SELECT * FROM '/var/log/**'` |
| One level (unchanged) | `SELECT * FROM './*'` | `SELECT * FROM './*'` or `SELECT * FROM './'` |
| Explicit glob (unchanged) | `SELECT * FROM './docs/**/*.md'` | `SELECT * FROM './docs/**/*.md'` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `'./'`, `'./*'` and any bare directory name return the files directly
  inside that directory only; files in subdirectories need `**`.
- The `no such table: files` hint reads `did you mean FROM './**'?`, and the
  hookless-`[[table]]` config error points at `FROM './**'`.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p docs/sub
touch top.md docs/a.md docs/sub/b.md
dirsql query "SELECT path FROM './' ORDER BY path"
# expected: [{"path":"top.md"}]
dirsql query "SELECT path FROM './**' ORDER BY path"
# expected: [{"path":"docs/a.md"},{"path":"docs/sub/b.md"},{"path":"top.md"}]
```
