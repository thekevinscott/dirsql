**Fixed** The CLI launcher now resolves package-name `[[dirsql.extension]]`
entries for **every** config flag in argv — `-c X`, `-c=X`, `-cX`,
`--config X`, `--config=X`, repeated flags included — instead of only the
first `--config`. Matches the Python launcher's semantics (#756); a config
passed via a short `-c` spelling, or any config after the first, no longer
has its package-name extensions silently skipped (#757).
