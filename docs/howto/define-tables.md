# Define tables for your files

Map a glob of files to a named SQL table so you query exactly the files you
care about, with exactly the columns you care about — instead of the
catch-all `files` table that [default mode](../reference/cli.md#default-mode)
serves.

## 1. Create a config next to your files

Suppose your blog posts live under `posts/`, one markdown file each. In the
directory you want to index, create a `.dirsql.toml` with one
[`[[table]]`](../reference/config.md#table) entry:

```toml
[[table]]
ddl  = "CREATE TABLE posts (path TEXT, size INTEGER, mtime INTEGER)"
glob = "posts/**/*.md"
```

- `glob` selects the files: every `.md` under `posts/`, at any depth,
  relative to the directory containing the config.
- `ddl` is a plain SQLite `CREATE TABLE` naming the columns you want. Here
  all three are [stat columns](../reference/columns.md#stat-columns) —
  filesystem facts `dirsql` computes for every file. Facts are opt-in by
  DDL: only the ones you declare become columns.

## 2. Query the table

Pass the config with [`-c`](../reference/cli.md#flags) — `dirsql` does not
auto-load a `.dirsql.toml` from the current directory. Each matched file is
one row:

```bash
dirsql query "SELECT path, size FROM posts ORDER BY path" -c ./.dirsql.toml
```

```json
[{"path":"posts/2024/hello.md","size":21},{"path":"posts/2025/again.md","size":55}]
```

Files that don't match the glob (a `README.txt` next to `posts/`, say) are
simply not in the table. Passing a config with `-c` fully replaces the
default `files` table — only the tables you define are served.

## Multiple tables

Add one `[[table]]` entry per table. When a file matches several globs, it
populates every matching table — each table is an independent view. See
[`[[table]]`](../reference/config.md#table) for that and the remaining keys
(`strict`, `on-file`).

## Going further

- Your directory layout encodes data (authors, dates, IDs)? Capture path
  segments as columns — [Derive columns from file paths](./columns-from-paths.md).
- Need columns from *inside* the files? A plain table never reads file
  contents — [Extract rows from file contents](./extract-from-contents.md).
- Why one row per file, rebuilt from disk? See
  [how `dirsql` thinks](../explanation.md).
