# Derive columns from file paths

Directory layouts often encode real data — an author, a year, a thread ID —
as path segments. An [`on-file`](../reference/config.md#table) hook receives
each file's path and can split it into columns, so a query can group and
filter on those segments.

## 1. Split the path in a hook

Suppose photos are filed by year and month:

```
photos/2024/05/beach.jpg
photos/2024/11/hike.jpg
photos/2025/01/snow.jpg
```

Write a small parser, `pathcols.py`, that turns the path into a row. It
receives the file's absolute path as its argument and prints a JSON array of
row objects:

```python
#!/usr/bin/env python3
import json, os, sys

parts = sys.argv[1].split(os.sep)
# .../photos/<year>/<month>/<file>
print(json.dumps([{"year": parts[-3], "month": parts[-2],
                   "basename": os.path.basename(sys.argv[1])}]))
```

Point a table at it in `.dirsql.toml`:

```toml
[[table]]
name = "photos"
ddl     = "CREATE TABLE photos (year TEXT, month TEXT, basename TEXT)"
glob    = "photos/*/*/*.jpg"
on-file = "python3 pathcols.py {path}"
```

The hook emits every column the table has — dirsql injects nothing. `{path}`
is the matched file's absolute path, one of the placeholders in the
[command hook contract](../reference/hooks.md#on-file). A column appears only
because the DDL declares it *and* the hook emits it.

## 2. Query the derived columns

Pass the config with [`-c`](../reference/cli.md#flags) (`dirsql` does not
auto-load a `.dirsql.toml` from the current directory):

```bash
dirsql query "SELECT year, month, basename FROM photos ORDER BY year, month" -c ./.dirsql.toml
```

```json
[{"basename":"beach.jpg","month":"05","year":"2024"},{"basename":"hike.jpg","month":"11","year":"2024"},{"basename":"snow.jpg","month":"01","year":"2025"}]
```

They are real SQL columns, so aggregation works:

```bash
dirsql query "SELECT year, COUNT(*) AS photos FROM photos GROUP BY year" -c ./.dirsql.toml
```

```json
[{"photos":2,"year":"2024"},{"photos":1,"year":"2025"}]
```

## Going further

- Only need the plain filesystem stat columns (`path`, `basename`, `dir`,
  `ext`, `size`, …) with no code? Query the path directly — a
  [path-table](../reference/path-tables.md) gives them for free, no config.
- The [tutorial](../getting-started.md) walks the same idea, deriving an
  author from the folder name, starting from zero.
- When the value you need lives inside the file rather than in its path,
  see [Extract rows from file contents](./extract-from-contents.md).
