**Removed**

Glob `{name}` placeholders no longer populate columns. `{name}` still matches like `*`, but a placeholder whose name is also a declared DDL column now rejects at load (the `ready` promise rejects), naming the placeholder and the fix. Populate the column from your `onFile` hook by splitting the path yourself.
