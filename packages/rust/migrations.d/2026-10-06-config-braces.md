### A braced name in a config glob is a literal

#### Summary

`glob = "data/{id}/metadata.json"` used to treat `{id}` as `*`. It now matches
a directory literally named `{id}`. `{a,b}` alternation is unchanged. A column
named like a `{name}` no longer errors at load.

#### Required changes

| Before | After |
| --- | --- |
| `glob = "data/{id}/metadata.json"` | `glob = "data/*/metadata.json"` |

#### Deprecations removed

- The `CaptureColumnCollision` error: a `{name}` is no longer a capture, so
  nothing can collide.

#### Behavior changes without code changes

- A config that kept `{name}` in a glob stops matching files and loads no rows.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir -p 'data/{id}' data/x
echo '[{"p":"lit"}]' > 'data/{id}/m.json'
echo '[{"p":"other"}]' > data/x/m.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "t"
glob = "data/{id}/m.json"
ddl = "CREATE TABLE t (p TEXT)"
on-file = "jq -sc add"
TOML
dirsql query "SELECT p FROM t" -c .dirsql.toml
# expected: [{"p":"lit"}]
```
