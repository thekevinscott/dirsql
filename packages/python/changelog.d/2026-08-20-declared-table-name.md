**Changed**

- **A table's name is now declared, not derived from `ddl`.** `Table` takes a required keyword-only `name` (`Table(name=..., ddl=..., glob=..., on_file=...)`), and a `[[table]]` entry in a `.dirsql.toml` requires a `name` key. The core no longer parses the DDL text to find the name; a `name` its `ddl` does not create fails at load with an error naming the entry. `Table.name` is now the declared `str` rather than an `Optional[str]` parsed from the DDL. (#962)
