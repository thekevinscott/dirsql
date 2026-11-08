### Core: config glob `.//x` anchors like `./x`

#### Summary

A config `[[table]] glob` with repeated separators after `./` (`.//a/*.json`)
matched no files; it now matches what `./a/*.json` matches. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
mkdir a
printf '{"t":"x"}' > a/x.json
printf '%s\n' '[[table]]' 'name = "dd"' 'ddl = "CREATE TABLE dd (t TEXT)"' 'glob = ".//a/*.json"' 'on-file = "cat"' > .dirsql.toml
dirsql "SELECT count(*) AS n FROM dd" -c .dirsql.toml --format json
# expected: [{"n":1}]
```
