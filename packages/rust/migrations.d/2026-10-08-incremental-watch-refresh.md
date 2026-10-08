### Watch refresh of an `on-file` table writes only changed rows

#### Summary

When a watch event re-runs an `on-file` table's command, dirsql keeps the
table's file list from the events instead of re-walking the glob, and
writes only the rows that differ from the stored ones. The command's
arguments and the row events are unchanged. Nothing breaks.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- After a refresh, rows that did not change keep their place and changed
  rows come last, so a query without `ORDER BY` may list rows in a
  different order than before. Row order was never defined.

#### Verification

```bash
cd "$(mktemp -d)"
echo '{"n":1}' > a.json
cat > .dirsql.toml <<'TOML'
[[table]]
name    = "t"
ddl     = "CREATE TABLE t (n INTEGER)"
glob    = "*.json"
on-file = "cat"
TOML
dirsql query "SELECT n FROM t" -c .dirsql.toml
# expected: one row, n = 1
```
