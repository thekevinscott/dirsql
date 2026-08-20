**Changed**

- **A table's name is now declared, not derived from `ddl`.** `TableDef` (and the `Table` class) gains a required `name`, and a `[[table]]` entry in a `.dirsql.toml` requires a `name` key. A `name` its `ddl` does not create fails at load with an error naming the entry. (#962)

**Removed**

- **The `parseTableName(ddl)` export.** It wrapped the core's DDL tokenizer, which no longer exists now that a table's name is declared rather than parsed. (#962)
