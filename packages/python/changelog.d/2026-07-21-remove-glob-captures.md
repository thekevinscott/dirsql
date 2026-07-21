**Removed**

Glob `{name}` placeholders no longer populate columns. `{name}` still matches like `*`, but a placeholder whose name is also a declared DDL column now raises at load (surfaced through `DirSQL.ready()`), naming the placeholder and the fix. Populate the column from your `on_file` hook by splitting the path yourself.
