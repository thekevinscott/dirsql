**Fixed** Columns now render in the order the SELECT list names them, in both
the table and JSON renderings. `SELECT path, ctime FROM './'` printed
`ctime, path` because the row maps are unordered and every renderer re-derived
its columns alphabetically. `Db::query_ordered` / `DirSQL::query_ordered`
return the projection order alongside the rows, and the CLI serializes each row
against it.
