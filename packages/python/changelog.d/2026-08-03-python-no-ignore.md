**Added** `DirSQL(..., no_ignore=True)`: a constructor opt-out from the
gitignore-by-default behavior path-table scans gained in #742, matching the
CLI's `--no-ignore` and the Rust builder's `DirSQLBuilder::no_ignore(bool)`.
The built-in `node_modules`/`.git` floor and configured `ignore` patterns
still apply. (#745)
