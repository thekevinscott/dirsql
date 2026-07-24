**Changed**

- **A `[[table]]` with no `on-file` hook is now a config-load error.** After the stat-fact injection layer was removed, a hook-less named table emitted no columns of its own, so every row was all-NULL. Loading such a config (`new DirSQL(configPath)` / `await db.ready`) now rejects, with a message pointing at the fix: add an `on-file` hook that emits the columns, or, for stat columns with no code, query the path directly with a path-table (`FROM './'`). This completes the fact-removal epic (#624). (#634)
