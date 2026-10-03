**Changed** `Table(glob=...)`, `DirSQL(ignore=...)` and the `[[table]] glob` /
`[dirsql] ignore` keys of a `.dirsql.toml` now follow the same rule as
path-tables: `*` and `?` match within one path segment and stop at `/`, `**`
matches any depth. `glob="*.json"` selects only the top-level `.json` files
and `ignore=["folder/*"]` hides only the files directly inside `folder/`.
Patterns written with `**` are unaffected.
