**Changed** `dirsql query` writes JSON rows as SQLite yields them instead of
building every row in memory first. `SELECT path FROM './*'` over a 1M-file
directory drops from 0.81s to 0.44s, and `SELECT path FROM './**/*.md'` over a
1.6M-file tree from 1.33s to 0.80s. The bytes printed are unchanged.
