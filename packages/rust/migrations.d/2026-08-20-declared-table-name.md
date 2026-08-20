### `[[table]]` and `Table` require an explicit `name` (#962)

#### Summary

A table's SQL name was derived by tokenizing the `CREATE TABLE` head of `ddl`
(`db::parse_table_name`) — bespoke SQL-as-text parsing that had to grow cases
for quoted identifiers, `schema.table`, `IF NOT EXISTS`, and leading comments.
The name is now **declared**: `[[table]]` requires a `name` key, and
`Table::new` / `Table::try_new` / `Table::strict` take it as their first
argument. After the DDL runs, SQLite's own catalog (`pragma_table_list`)
settles whether a table by that name exists; if not, the build fails before any
file is ingested. `Db::create_table` takes `(name, ddl)`, and
`db::parse_table_name` is deleted.

#### Required changes

| Before | After | Fix |
| ------ | ----- | --- |
| `[[table]]` with `ddl` + `glob` + `on-file` | Load fails: `Missing required field 'name' in [[table]] entry` | Add `name = "<table>"`, matching the table the `ddl` creates. |
| `Table::new(ddl, glob, on_file)` | Does not compile | `Table::new(name, ddl, glob, on_file)`; same for `try_new` / `strict`. |
| `Db::create_table(ddl)` | Does not compile | `Db::create_table(name, ddl)`. |
| `dirsql::db::parse_table_name(&ddl)` | Does not compile | Use the name you declared. The catalog (`pragma_table_list` / `sqlite_master`) answers "does this table exist". |
| `DirSqlError::Ddl` returned for a DDL whose name could not be parsed | No longer produced for that case | Match `DirSqlError::Core(_)` for a DDL SQLite rejects, and the new `DirSqlError::TableNotCreated { name }` for a `name` the DDL never creates. |

#### Deprecations removed

- `dirsql::db::parse_table_name` — removed outright, with the
  `strip_keyword_ci` / `parse_identifier` / `skip_ws_comments` tokenizer
  helpers behind it.

#### Behavior changes without code changes

- A `[[table]]` whose `ddl` creates a table under a different name than its
  `name` is now a hard load error (`table 'messages': its `ddl` ran but created
  no table called 'messages'. Set `name` to the table the `ddl` creates.`).
  Previously the parsed name always agreed with the DDL by construction, so
  this mismatch could not arise.
- Duplicate-table detection across composed configs compares the declared
  `name` keys rather than parsed DDL names. A config whose DDL was previously
  unparseable took part in no collision check; every entry now does.
- Quoted (`CREATE TABLE "messages"`), schema-qualified (`main.messages`) and
  `IF NOT EXISTS` DDL keep working unchanged — SQLite records the bare
  identifier in its catalog, so a plain `name = "messages"` matches.

#### Verification

```bash
# .dirsql.toml
#   [[table]]
#   name    = "records"
#   ddl     = "CREATE TABLE records (id TEXT)"
#   glob    = "data/*.json"
#   on-file = "cat {path}"

dirsql query "SELECT id FROM records" -c ./.dirsql.toml
# -> [{"id":"one"},{"id":"two"}]

# Drop the `name` key:
dirsql query "SELECT id FROM records" -c ./.dirsql.toml
# -> error: Missing required field 'name' in [[table]] entry

# Set name = "messages" while the ddl still creates `records`:
dirsql query "SELECT id FROM messages" -c ./.dirsql.toml
# -> error: table 'messages': its `ddl` ran but created no table called 'messages'.
```
