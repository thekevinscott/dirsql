**Changed**

- **One equality lookup no longer indexes every row.** `WHERE path = '...'` (or `basename` / `dir`) on a path-table compares each row once instead of building a lookup index over the whole table; a join that probes the same column again still gets the index. Output is unchanged.
