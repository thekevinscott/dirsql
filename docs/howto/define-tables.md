# Define tables for your files

Map a glob of files to a named SQL table so you query exactly the files you
care about, with exactly the columns you care about — instead of the
catch-all `files` table that [zero-config mode](../reference/cli.md#zero-config-mode)
serves.

## 1. Create a config next to your files

Suppose your blog posts live under `posts/`, one markdown file each. In the
directory you want to index, create a `.dirsql.toml` with one
[`[[table]]`](../reference/config.md#table) entry:

```toml
[[table]]
ddl  = "CREATE TABLE posts (_path TEXT, _size INTEGER, _mtime INTEGER)"
glob = "posts/**/*.md"
```

- `glob` selects the files: every `.md` under `posts/`, at any depth,
  relative to the directory containing the config.
- `ddl` is a plain SQLite `CREATE TABLE` naming the columns you want. Here
  all three are [virtual columns](../reference/columns.md#virtual-columns) —
  filesystem facts `dirsql` computes for every file. Facts are opt-in by
  DDL: only the ones you declare become columns.

## 2. Query the table

Each matched file is one row:

```bash
dirsql query "SELECT _path, _size FROM posts ORDER BY _path"
```

```json
[{"_path":"posts/2024/hello.md","_size":21},{"_path":"posts/2025/again.md","_size":55}]
```

Files that don't match the glob (a `README.txt` next to `posts/`, say) are
simply not in the table. Once a config file exists, it fully replaces the
zero-config default — only the tables you define are served.

## Multiple tables

Add one `[[table]]` entry per table. When a file matches several globs, the
first matching table wins — see [`[[table]]`](../reference/config.md#table)
for that and the remaining keys (`strict`, `on-file`).

## Going further

- Your directory layout encodes data (authors, dates, IDs)? Capture path
  segments as columns — [Derive columns from file paths](./columns-from-paths.md).
- Need columns from *inside* the files? A plain table never reads file
  contents — [Extract rows from file contents](./extract-from-contents.md).
- Why one row per file, rebuilt from disk? See
  [how `dirsql` thinks](../explanation.md).
