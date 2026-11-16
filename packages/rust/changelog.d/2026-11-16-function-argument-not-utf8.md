**Fixed** Calling a `[[dirsql.function]]` with an argument holding text that is
not valid UTF-8 (for example `up(CAST(x'ff41' AS TEXT))`) now fails the query
with `argument <n> of "<function>" holds text that is not valid UTF-8`. It
used to panic, failing the query with `unwinding panic`.
