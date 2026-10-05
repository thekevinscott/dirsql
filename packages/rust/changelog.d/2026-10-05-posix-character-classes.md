**Fixed** POSIX character classes inside brackets match as they do in bash:
`[[:digit:]]`, `[[:upper:]]`, `[![:alpha:]]` and the rest of `alnum`, `alpha`,
`ascii`, `blank`, `cntrl`, `digit`, `graph`, `lower`, `print`, `punct`,
`space`, `upper`, `word` and `xdigit`. They used to match nothing. Applies to
path-tables and to declared `[[table]]` globs and `ignore` patterns. Classes
cover ASCII characters.
