**Changed**

- **A `[[table]]` with no `on-file` hook is now a load error.** After the stat-fact injection layer was removed, a hook-less named table emitted no columns of its own, so every row was all-NULL — useless and surprising. `load_config_str` (and every path that loads a config, including `DirSQL::from_config_path` and the CLI's `-c`) now rejects such a table with a message pointing at the fix: add an `on-file` hook that emits the columns, or, for stat columns with no code, query the path directly with a path-table (`FROM './'`). This completes the fact-removal epic (#624). (#634)
