**Fixed** A path-table whose path ends in whitespace (`FROM './trail '`,
`FROM './dir '`) now resolves to that exact file or directory. It used to fail
with `no such table`, because the trailing whitespace was trimmed from the name.
