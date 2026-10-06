**Changed** Path-table scans read each directory's names into one shared
buffer instead of allocating per entry. `SELECT count(*) FROM './*'` over a
1M-file directory drops from 0.62s to 0.39s, and `SELECT path` from 1.05s to
0.82s. The files returned and their order are unchanged.
