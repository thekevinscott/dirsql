**Fixed** A path-table whose glob has no `**` no longer walks the subtree
below the depth its pattern can match. `'./docs/*.md'` and `'./docs/a.md'`
list `docs` and nothing beneath it, and `'~/.bashrc'` lists the home
directory instead of walking all of it. Rows are unchanged.
