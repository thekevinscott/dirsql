# Define tables for your files

Map a glob of files to a named SQL table so you query exactly the files you
care about, by name — a shape you can reuse, keep live with the watcher, and
persist across restarts, instead of repeating an ad-hoc
[path-table](../reference/path-tables.md) path in every query.

## 1. Create a config with a table

Suppose your blog posts live under `posts/`, one markdown file each. A named
table needs three keys: a `glob` that selects the files, a `ddl` that names
the columns, and an [`on-file`](../reference/config.md#table) hook that emits
each file's rows. Put a small parser next to the config — `extract.py`, which
reads a post's title line and prints a JSON array of row objects:

```python
#!/usr/bin/env python3
import json, os, sys

text = open(sys.argv[1], encoding="utf-8").read()
title = next((l[2:].strip() for l in text.splitlines() if l.startswith("# ")), None)
print(json.dumps([{"title": title, "slug": os.path.basename(sys.argv[1])[:-3]}]))
```

Then declare the table in `.dirsql.toml`:

```toml
[[table]]
name = "posts"
ddl     = "CREATE TABLE posts (title TEXT, slug TEXT)"
glob    = "posts/**/*.md"
on-file = "python3 extract.py {path}"
```

- `glob` selects the files: every `.md` under `posts/`, at any depth, relative
  to the directory containing the config.
- `ddl` is a plain SQLite `CREATE TABLE` naming the columns you want to keep.
- `on-file` is **required** — it is where the table's rows come from. dirsql
  injects nothing; the hook emits every column, reading the file (it has
  `{path}`) and deriving whatever it needs. A `[[table]]` with no `on-file` is
  a [config error](../reference/config.md#parse-errors). For plain stat
  columns with no code, query the path directly with a path-table instead.

## 2. Query the table

Pass the config with [`-c`](../reference/cli.md#flags) — `dirsql` does not
auto-load a `.dirsql.toml` from the current directory. Each matched file is
one row:

```bash
dirsql query "SELECT title, slug FROM posts ORDER BY slug" -c ./.dirsql.toml
```

```json
[{"slug":"again","title":"On Recursion"},{"slug":"hello","title":"Hello World"}]
```

Files that don't match the glob (a `README.txt` next to `posts/`, say) are
simply not in the table. Only the tables you define are served.

## Multiple tables

Add one `[[table]]` entry per table — each with its own `glob`, `ddl`, and
`on-file`. When a file matches several globs, it populates every matching
table — each table is an independent view. See
[`[[table]]`](../reference/config.md#table) for the remaining key, `strict`.

## Going further

- The parser mechanics — placeholders, stdout protocol, per-file failure
  isolation — are the [`on-file` hook contract](../reference/hooks.md#on-file);
  [Extract rows from file contents](./extract-from-contents.md) is the fuller
  recipe.
- Your directory layout encodes data (authors, dates, IDs)? Split the path in
  the hook — [Derive columns from file paths](./columns-from-paths.md).
- Why one row per file, rebuilt from disk? See
  [how `dirsql` thinks](../explanation.md).
