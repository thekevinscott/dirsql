### `on-file` runs execute concurrently

#### Summary

When a table's matched paths outgrow one argument list, dirsql splits them
into runs and now executes the runs concurrently, one per CPU at most. Rows
keep their order within a run, but not across runs.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A query that relied on rows coming back in path order across runs must add
  an `ORDER BY`. A table small enough for one run is unchanged.
- A hook that holds memory or a GPU may now have several copies running at
  once on a very large table.

#### Verification

```bash
cd "$(mktemp -d)"
for i in $(seq 1 5000); do : > "a-long-file-name-to-fill-the-argument-list-$i.txt"; done
cat > count.sh <<'SH'
echo "{\"n\":$#}"
SH
cat > .dirsql.toml <<'TOML'
[[table]]
name    = "t"
ddl     = "CREATE TABLE t (n INTEGER)"
glob    = "*.txt"
on-file = "sh count.sh"
TOML
dirsql query "SELECT count(*) AS runs, sum(n) AS paths FROM t" -c .dirsql.toml
# expected: one row whose paths is 5000 and whose runs is greater than 1
```
