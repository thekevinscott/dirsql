**Fixed**

- **Path-table scans now skip `node_modules` and `.git` at any depth.** The
  built-in `DEFAULT_IGNORES` were anchored at the table root
  (`node_modules/**`, `.git/**`), so nested occurrences such as
  `presentations/foo/node_modules/...` leaked into `SELECT ... FROM './'`
  results. The defaults are now `**/node_modules/**` and `**/.git/**`, and
  the directory walker prunes wholly-ignored directories during traversal
  instead of filtering after the walk, so an ignored tree is never read at
  all. Naming a skipped directory outright (e.g.
  `FROM './apps/site/node_modules'`) still scans it, at any depth. (#741)
