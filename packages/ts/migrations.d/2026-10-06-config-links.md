### Config globs skip node_modules unless named, and follow symlinks

#### Summary

`glob = "**/*.js"` used to list files under any `node_modules`; it now does
not, unless a component of the glob is `node_modules`. Symlinked files and
directories, which a config glob used to ignore, are now followed as in a
path-table and in bash.

#### Required changes

| Before | After |
| --- | --- |
| `glob = "**/*.js"` (included `node_modules/`) | `glob = "node_modules/**/*.js"` to list it |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A glob with no `node_modules` component never matches under one.
- A symlinked file or directory the glob reaches now yields rows.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir real node_modules
echo '[{"p":"r"}]' > real/r.json
echo '[{"p":"n"}]' > node_modules/n.json
ln -s real linkdir
cat > .dirsql.toml <<'TOML'
[[table]]
name = "t"
glob = "**/*.json"
ddl = "CREATE TABLE t (p TEXT)"
on-file = "jq -sc add"
TOML
dirsql query "SELECT p FROM t" -c .dirsql.toml
# expected: no row from node_modules; rows for real/ (symlinked linkdir/ is
# reached by `linkdir/*.json`, not by `**`)
```
