**Changed**

- **A plain projection of a path table, such as `SELECT path FROM './a/b/**/*.md'`, reads its rows straight from the walk instead of stepping SQLite once per row.** Any `WHERE`, `ORDER BY`, `LIMIT`, expression, join or `content` column still runs through SQLite. Query results are unchanged.
