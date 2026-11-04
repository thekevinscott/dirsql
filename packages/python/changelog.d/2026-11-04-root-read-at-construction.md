**Fixed**

- **`DirSQL` reads its root when constructed.** The default root (the cwd) and a relative `root` were resolved when the background scan started, after the constructor returned, so a `chdir` right after `DirSQL(...)` indexed the new directory. Both now resolve to an absolute path in the constructor.
