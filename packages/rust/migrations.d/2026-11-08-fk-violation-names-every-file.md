### Rust: `ForeignKeyViolation` lists every offender

#### Summary

`dirsql::foreign_key::ForeignKeyViolation` reported only the first offending
row's table and file. It now carries `offenders`, every distinct table and file
holding an offending row, and its message names each of them even past the
ten rows it lists. Breaking for Rust code that reads the removed `table` or
`file` fields; the Python and TypeScript SDKs and the CLI change only the error
text of a violation with more than ten offending rows.

#### Required changes

| Before | After |
| --- | --- |
| `violation.table` | `violation.offenders[0].table` |
| `violation.file` | `violation.offenders[0].file` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- A violation with more than ten offending rows ends
  ``(and N more, from `<file>`, table `<table>`)``, naming each file, or table
  for rows no single file owns, not already named among the listed rows.

#### Verification

```bash
cd "$(mktemp -d)"
mkdir parents kids
echo '{"id": 1}' > parents/1.json
seq 2 12 | sed 's/.*/{"pid": &}/' > kids/a.jsonl
seq 50 61 | sed 's/.*/{"pid": &}/' > kids/b.jsonl
cat > .dirsql.toml <<'TOML'
[[table]]
name = "parent"
ddl  = "CREATE TABLE parent (id INTEGER PRIMARY KEY)"
glob = "parents/*.json"
on-file = "cat"

[[table]]
name = "child"
ddl  = "CREATE TABLE child (pid INTEGER REFERENCES parent(id))"
glob = "kids/a.jsonl"
on-file = "cat"

[[table]]
name = "pet"
ddl  = "CREATE TABLE pet (pid INTEGER REFERENCES parent(id))"
glob = "kids/b.jsonl"
on-file = "cat"
TOML
dirsql query "SELECT 1" -c .dirsql.toml
# expected: exits 1; the message lists ten `pet` rows and ends
# (and 13 more, from table `child`)
```
