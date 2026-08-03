**Fixed** The CLI launcher now resolves package-name `[[dirsql.extension]]`
entries for **every** config flag in argv — `-c X`, `-c=X`, `-cX`,
`--config X`, `--config=X`, repeated flags included — instead of only the
first `--config`. In particular, config fragments injected by plugin
discovery (which arrive as `-c` flags) get their package-name extensions
resolved to literal paths, so a plugin declaring e.g. `path = "sqlite_vec"`
loads on the pure discovery path instead of failing with
`failed to load extension` (#754).
