# Derive columns from file paths

Directory layouts often encode real data — an author, a year, a thread ID —
as path segments. A `{name}` capture in a table's glob turns such a segment
into a queryable column, no extraction code required.

## 1. Name the segment in the glob

Suppose photos are filed by year and month:

```
photos/2024/05/beach.jpg
photos/2024/11/hike.jpg
photos/2025/01/snow.jpg
```

Capture both directory levels in `.dirsql.toml`:

```toml
[[table]]
ddl  = "CREATE TABLE photos (year TEXT, month TEXT, basename TEXT)"
glob = "photos/{year}/{month}/*.jpg"
```

A capture only populates a column when the DDL declares one with the same
name — here `year` and `month`. The capture rules (valid names, matching
within one path segment) are in
[glob captures](../reference/columns.md#glob-captures).

## 2. Query the captured columns

Pass the config with [`-c`](../reference/cli.md#flags) (`dirsql` does not
auto-load a `.dirsql.toml` from the current directory):

```bash
dirsql -c ./.dirsql.toml query "SELECT year, month, basename FROM photos ORDER BY year, month"
```

```json
[{"basename":"beach.jpg","month":"05","year":"2024"},{"basename":"hike.jpg","month":"11","year":"2024"},{"basename":"snow.jpg","month":"01","year":"2025"}]
```

Captures are real SQL columns, so aggregation works:

```bash
dirsql -c ./.dirsql.toml query "SELECT year, COUNT(*) AS photos FROM photos GROUP BY year"
```

```json
[{"photos":2,"year":"2024"},{"photos":1,"year":"2025"}]
```

## Going further

- Captures combine freely with [stat columns](../reference/columns.md#stat-columns)
  (`basename` above) — both are filesystem facts merged onto every row.
- The [tutorial](../getting-started.md) walks the same idea with an
  `{author}` capture, starting from zero.
- When the value you need lives inside the file rather than in its path,
  see [Extract rows from file contents](./extract-from-contents.md).
