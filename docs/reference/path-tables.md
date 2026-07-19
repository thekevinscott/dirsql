# Path-tables

A **path-table** is a table you never declare. Write a path where a table name
goes, and dirsql scans the filesystem for you:

```sql
SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5;
```

No `.dirsql.toml` entry, no DDL, no `on_file`. The name *is* the query.

## How resolution works

dirsql never parses your SQL. It hands the statement to SQLite untouched; only
when SQLite reports `no such table: X` does dirsql look at `X`:

1. If `X` starts with `./`, dirsql registers a path-table over the matching
   files and re-prepares the statement.
2. Otherwise the SQLite error stands, unchanged.

Because discovery rides on SQLite's own errors, joins, subqueries and CTEs work
with no extra machinery — SQLite names each missing target in turn, and dirsql
resolves them one at a time.

Two consequences follow directly:

- **A declared table always wins.** The fallback only runs after SQLite has
  already failed to find the name, so a real table is found first. A table you
  genuinely named `"./"` shadows the path, and dirsql will not argue.
- **A typo stays a typo.** `SELECT * FROM usrs` fails with SQLite's error and
  nothing else. dirsql never guesses that an ordinary identifier meant a file.

## Writing the path

The path is relative to the **index root** — the directory dirsql is indexing,
not your shell's working directory.

| You write | dirsql scans |
| --- | --- |
| `'./'` | every file under the index root, recursively |
| `'./docs/*.md'` | markdown files directly inside `docs/` |
| `'./docs/**/*.md'` | markdown files at any depth under `docs/` |

The `./` is required. A bare glob is rejected with a hint rather than silently
accepted:

```
SELECT * FROM '**/*.md';
-- no such table: **/*.md; did you mean './**/*.md'?
```

Absolute (`/var/log/*.log`), parent-relative (`../notes`) and home-relative
(`~/notes`) path-tables are recognized but not yet resolved; they report that
they are unsupported rather than returning wrong rows.

## Columns

A path-table has the same [virtual columns](/reference/columns) as any dirsql
table:

| Column | Type | Meaning |
| --- | --- | --- |
| `path` | TEXT | path relative to the index root |
| `basename` | TEXT | filename with extension |
| `dir` | TEXT | parent directory, relative to the index root |
| `ext` | TEXT | extension without the dot |
| `size` | INTEGER | size in bytes |
| `mtime` | INTEGER | modification time, Unix seconds |
| `ctime` | INTEGER | creation/change time, Unix seconds |

There is also a hidden `content` column holding the file's text. It is excluded
from `SELECT *` and read only when you name it, so scanning a large tree costs
nothing until you ask for file bodies:

```sql
SELECT path FROM './docs/*.md' WHERE content LIKE '%deprecated%';
```

A file that cannot be read, or is not valid UTF-8, yields `NULL` content rather
than failing the query.

## Freshness and scope

A path-table is scanned when the statement runs, so it always reflects the
filesystem as it is *now* — unlike declared tables, which are indexed on build
and updated by the watcher. A file created a moment ago shows up immediately.

Path-tables are per-connection and are never written to a persistent cache, so
they cannot leak into `sqlite_master` or survive a restart. The reserved
top-level `.dirsql/` directory is excluded from the scan, as everywhere else.

## Joining against declared tables

Path-tables are ordinary SQLite tables once resolved, so they join freely:

```sql
SELECT p.basename, f.size
FROM './docs/*.md' AS p
JOIN files AS f ON f.path = p.path;
```

A zero-match path-table is not an error — it is an empty table, and the query
returns no rows.
