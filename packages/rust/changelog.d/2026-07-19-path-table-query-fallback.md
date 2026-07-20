**Added** — path-tables in `query()`. A table name SQLite does not know, but
which begins with `./`, now resolves to a live glob scan of the index root:
`SELECT basename, size FROM './' ORDER BY size DESC` works with no DDL and no
config. Discovery rides on SQLite's own `no such table` error, so joins,
subqueries and CTEs resolve several path-tables in one statement, and a
declared table always wins. A bare glob (`'**/*.md'`) is rejected with a
`did you mean './**/*.md'?` hint rather than silently accepted; an ordinary
typo fails with SQLite's error, unchanged. Absolute, `../` and `~/` path-tables
are recognized but report that they are not yet resolved. All three SDKs and
the CLI inherit this from the core.
