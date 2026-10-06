**Changed** Path-table queries stat a file only when the statement reads
`size`, `mtime`, `ctime` or `content`. A query over `path`, `basename`, `dir`
or `ext` alone no longer stats every matched file.
