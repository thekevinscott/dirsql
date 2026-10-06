**Changed** Path-table scans and the startup scan read sibling directories on
spare cores. `SELECT count(*) FROM './**/*.md'` over a 1.6M-file tree spread
across 2.3k directories drops from 0.72s to 0.23s, and `SELECT path` from
0.80s to 0.33s. The files returned and their order are unchanged.
