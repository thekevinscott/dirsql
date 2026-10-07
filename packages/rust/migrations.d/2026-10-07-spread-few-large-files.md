### `on-file` splits large tables by file size

#### Summary

A table whose paths fit one argument list is now also split into several
concurrent runs when its files hold at least 4 MiB in total per run, up
to one run per CPU. Small tables are unchanged.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A hook may now see a subset of the table's paths even when the table is
  small in path count. A hook must already not assume it sees every path;
  do cross-file work in SQL.
- Rows from different runs have no defined order; add an `ORDER BY`.

#### Verification

```bash
cd "$(mktemp -d)"
for i in 1 2 3 4; do head -c 5000000 /dev/zero | tr '\0' x > "f$i.txt"; done
printf 'echo "{\\"n\\":$#}"\n' > count.sh
cat > .dirsql.toml <<'TOML'
[[table]]
name    = "t"
ddl     = "CREATE TABLE t (n INTEGER)"
glob    = "*.txt"
on-file = "sh count.sh"
TOML
dirsql query "SELECT count(*) AS runs, sum(n) AS paths FROM t" -c .dirsql.toml
# expected: paths is 4; runs is the smaller of 4 and the CPU count
```
