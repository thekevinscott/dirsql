**Changed** `[[table]] glob` and `[dirsql] ignore` patterns now follow the
same rule as path-tables: `*` and `?` match within one path segment and stop
at `/`, `**` matches any depth. `glob = "*.json"` selects only the top-level
`.json` files, `glob = "folder/*"` no longer reaches `folder/sub/`, and
`ignore = ["folder/*"]` hides only the files directly inside `folder/`. A
`{name}` placeholder matches exactly one segment. Patterns written with `**`
(including the built-in `**/node_modules/**` and `**/.git/**` skips) are
unaffected. Applies to the SDK `tables` and `ignore` parameters alike.
