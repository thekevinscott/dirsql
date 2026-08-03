**Changed** Path-table scans (`SELECT ... FROM './'`) now respect
`.gitignore` files by default, the way fd/ripgrep do: a `.gitignore` anywhere
in the tree applies below its own directory (hierarchically, `!pattern`
re-includes), and ignored directories are pruned rather than walked. No
`.git` directory is required. Hidden files are still scanned (deliberate
divergence from fd/rg), the built-in `node_modules`/`.git` defaults remain as
a floor, and naming a gitignored directory outright (`FROM './dist'`) still
scans it. The new CLI flag `--no-ignore` and Rust builder option
`DirSQLBuilder::no_ignore(bool)` restore the full walk. (#742)
