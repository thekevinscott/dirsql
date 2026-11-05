**Fixed**

- **An `--on-file` path-table infers one column for keys that differ only in ASCII case.** Rows emitting `title` and `Title` failed the query with `vtable constructor failed`; they now share one column, named by the first-seen spelling and typed across every spelling.
