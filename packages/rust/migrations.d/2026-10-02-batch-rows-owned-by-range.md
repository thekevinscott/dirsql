### Core: a per-table hook's rows are owned as one rowid range

#### Summary

A table fed by an `on-file` command no longer records one
`_dirsql_internal_rows` entry per ingested row. The batch is owned as a single
`(table, first_rowid, last_rowid)` range in a new internal table,
`_dirsql_internal_ranges`, created alongside `_dirsql_internal_rows` on every
connection, including an existing persisted cache. No signature, config key
or CLI flag changes.

#### Required changes

_None._

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `_dirsql_internal_ranges` joins the reserved `_dirsql_*` namespace: a
  `query()` that reads it is denied like the other internal tables.
- A persisted cache gains the table on next open; a cold rebuild clears it
  with the rest of the bookkeeping.
- Rows, their order, `rowid`s and query results are unchanged.

#### Verification

```bash
cd "$(mktemp -d)"
printf 'a\n' > a.txt
printf 'b\n' > b.txt
cat > .dirsql.toml <<'TOML'
[[table]]
name = "items"
ddl = "CREATE TABLE items (name TEXT)"
glob = "*.txt"
on-file = "sh -c 'printf \"[\"; sep=\"\"; for f in \"$@\"; do printf \"%s{\\\"name\\\":\\\"%s\\\"}\" \"$sep\" \"$(basename \"$f\")\"; sep=\",\"; done; printf \"]\\n\"' sh"
TOML
dirsql query "SELECT name FROM items ORDER BY name" -c .dirsql.toml
# expected: [{"name":"a.txt"},{"name":"b.txt"}]
dirsql query "SELECT * FROM _dirsql_internal_ranges" -c .dirsql.toml; echo "exit: $?"
# expected: a "not authorized" error and a non-zero exit
```
