# Columns

Where a dirsql table's columns come from depends on which kind of table it is.
The rule that governs both is the same: **dirsql never injects a column your
table did not produce.** There is no automatic path/size/mtime merge, and glob
`{name}` segments capture nothing.

## Named tables: exactly the hook's output

A named [`[[table]]`](./config.md#table) — or an SDK
[`Table`](./sdk.md#table) — has exactly the columns its
[`on-file` hook](./hooks.md#on-file) emits, narrowed to the columns the DDL
declares. dirsql adds nothing on top: no `path`, no `size`, no value derived
from the filename. A hook that wants any of those computes them itself. The
hook receives the file's `{path}` (an SDK `on_file` callback receives the same
path as its argument) and may stat or read the file however it likes.

The hook is **required**. A `[[table]]` with no `on-file` is a
[config-load error](./config.md#parse-errors): with nothing supplying columns,
every row would be all-NULL. The error points at the fix — add a hook, or, for
plain stat columns with no code, query the path directly with a path-table.

## Path-tables: stat columns

A [path-table](./path-tables.md) (`FROM './'`) is the one place dirsql supplies
columns for you. Each matched file becomes one row carrying seven **stat
columns** — derived from the file's path and its `stat` metadata — plus a
lazily-read hidden [`content`](./path-tables.md#columns) column.

### Stat columns

| Column | Type | Value |
|---|---|---|
| `path` | TEXT | The file's path relative to the scan root (e.g. `posts/hello.md`); absolute for a `/`, `../` or `~/` path-table. |
| `basename` | TEXT | The filename, including extension (`hello.md`). |
| `dir` | TEXT | The parent directory relative to the root (`posts`); the empty string for files directly under the root. |
| `ext` | TEXT | The file extension without the leading dot (`md`). Original case is preserved — `Photo.JPG` yields `JPG`; use `LOWER(ext)` for case-insensitive matching. `NULL` when the file has no extension. |
| `size` | INTEGER | File size in bytes. |
| `mtime` | INTEGER | Last-modified time, Unix seconds. |
| `ctime` | INTEGER | Creation (birth) time, Unix seconds. `NULL` when the platform or filesystem cannot supply it. |

These are ordinary stored `TEXT`/`INTEGER` values, computed once per file at
scan time — not SQLite `GENERATED ... VIRTUAL` columns and not part of a
`CREATE VIRTUAL TABLE`. "Stat" describes where the value comes from — the
file's path and `stat` metadata, as opposed to its content — not how it is
stored.

A stat value that cannot be computed (an unreadable file's `size`, a missing
extension's `ext`) is `NULL`.

```sql
SELECT basename, size
FROM './posts'
WHERE mtime > strftime('%s', '2024-01-01')
ORDER BY mtime DESC;
```

[Attaching a parser](./path-tables.md#parsing-rows-with-on-file) to a
path-table with `--on-file` **replaces** these stat columns with the parser's
own output — the two modes stay cleanly separate, exactly as for a named
table. A parser that wants the path emits it; it has `{path}`.

## Deriving columns from the path

To turn path segments (an author, a year, a thread id) into columns, a hook
splits `{path}` and emits the pieces — the same as any other column it
produces. dirsql does not do this for you: a `{name}` segment in a glob is
rewritten to `*` and matches a single path segment, but captures no value.
[Derive columns from file paths](../howto/columns-from-paths.md) walks a
worked example.
