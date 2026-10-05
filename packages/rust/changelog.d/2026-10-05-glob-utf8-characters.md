**Fixed** `?` and bracket expressions in path-tables and `[[table]]` globs
match one character, as bash does in a UTF-8 locale, instead of one byte.
`'./caf?.md'` now lists `café.md`, `'./na[ï]ve.txt'` lists `naïve.txt`, and
`'./na??ve.txt'` no longer lists `naïve.txt`.
