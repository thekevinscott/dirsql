### Core: a path-table path keeps its trailing whitespace

#### Summary

A path-table naming a file or directory whose name ends in whitespace used to
fail with `no such table`; it now resolves to that path, as the glob forms
already did. Affects every query surface (CLI, SDKs, server). Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._

#### Verification

```bash
cd "$(mktemp -d)"
printf 's\n' > 'trail '
dirsql "SELECT path FROM './trail '" --format json
# expected: [{"path":"trail "}]
```
