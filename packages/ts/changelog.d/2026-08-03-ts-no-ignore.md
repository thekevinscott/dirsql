**Added**

`new DirSQL({ noIgnore: true })` opts path-table scans out of their default
`.gitignore` respect (#742), restoring the gitignored files to scan results —
the TypeScript equivalent of the CLI's `--no-ignore` and the Rust builder's
`.no_ignore(bool)` (#746). The built-in floor (`node_modules`, `.git`) and any
configured `ignore` patterns still apply either way. Default behavior is
unchanged.
