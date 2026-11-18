**Fixed**

- **`DirSQL` reads relative `extensions` paths when constructed.** A relative programmatic extension path (and a bare name's same-named local file) resolved against the cwd when the background build started, after the constructor returned, so a `chdir` right after `DirSQL(...)` loaded the extension from the new directory. They now resolve against the cwd at construction.
