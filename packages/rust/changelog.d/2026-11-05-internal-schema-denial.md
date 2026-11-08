**Fixed** `query()` now denies every schema read of the internal `_dirsql_*`
tables, not just a lowercase `PRAGMA`: an upper- or mixed-case `PRAGMA`
argument, a `pragma_*` table-valued function such as
`pragma_table_info('_dirsql_internal_rows')`, and `dbstat` are all rejected
as not authorized.
