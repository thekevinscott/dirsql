### CLI: `--format table` names an infinite REAL

#### Summary

`--format table` printed an infinite REAL as `NULL`; it now prints `inf` or
`-inf`. Affects the one-shot query and the REPL in table format only. JSON
output is unchanged. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A table cell holding `+Infinity` / `-Infinity` reads `inf` / `-inf` rather
  than `NULL`.

#### Verification

```bash
cd "$(mktemp -d)"
dirsql --format table "SELECT 9e999 AS p, -9e999 AS m"
# expected:
# p    m
# ---  ----
# inf  -inf
#
# 1 row
```
