### A config glob hides dot-named entries unless it spells them

#### Summary

`glob = "**/*.md"` used to match `.env.md` and everything under `.hid/`. It
now matches neither, as a path-table and bash (with `dotglob` off) do. A config
that read dot-named files without naming them loses those rows.

#### Required changes

| Before | After |
| --- | --- |
| `glob = "**/*.md"` (included `.hid/z.md`) | `glob = ".hid/*.md"` or `glob = "{*,.hid}/*.md"` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A dot-named component matches only where the glob spells a dot (`.hid`,
  `.*`).
- A glob that names the dot directory (`.hid/*.md`) is unchanged.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir .hid
echo '[{"p":"a"}]' > a.json
echo '[{"p":"h"}]' > .hid/h.json
cat > .dirsql.toml <<'TOML'
[[table]]
name = "t"
glob = "**/*.json"
ddl = "CREATE TABLE t (p TEXT)"
on-file = "jq -sc add"
TOML
dirsql query "SELECT p FROM t" -c .dirsql.toml
# expected: [{"p":"a"}]
```
