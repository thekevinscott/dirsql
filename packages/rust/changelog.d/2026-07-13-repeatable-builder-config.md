**Added**

- **The builder and core now accept multiple config files, merged in order.** `DirSQLBuilder::config(path)` is repeatable — each call appends onto an ordered list. `[[table]]`, `ignore`, and `[[dirsql.extension]]` entries accumulate across configs in call order; each config's `on-file` hooks run from that config file's own directory under its own `[dirsql].hook-timeout`; a duplicate table name across configs errors (`DuplicateTable`). A single config is byte-identical to before. (#553, #545)
