### A config glob naming a directory lists its files

#### Summary

`glob = "docs"` used to match nothing, because no file is named `docs`. It now
lists the files directly inside `docs`, as `FROM './docs'` does. A config that
relied on the empty result gains rows.

#### Required changes

| Before | After |
| --- | --- |
| `glob = "docs"` (no rows) | `glob = "docs"` lists `docs/*`; write `docs/**` for every depth |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A glob with no wildcard that names a directory expands to `<dir>/*`.
- A glob with no wildcard that names a file still names only that file.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir docs
echo '[{"p":"a"}]' > docs/a.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "t"
glob = "docs"
ddl = "CREATE TABLE t (p TEXT)"
on-file = "jq -sc add"
TOML
dirsql query "SELECT p FROM t" -c .dirsql.toml
# expected: [{"p":"a"}]
```
