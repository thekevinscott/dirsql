**Fixed**

- **A relative `root` is resolved once, when the instance is built.** Path-tables no longer re-join a relative root to the current working directory on every query, so a `chdir` after the index is built no longer empties them or reads a same-named directory under the new cwd. `DirSQL::root()` now returns the absolute path.
