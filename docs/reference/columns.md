# Virtual columns and glob captures

Every table — config-defined or programmatic — gets filesystem facts merged
onto its rows automatically: seven reserved **virtual columns** derived from
the file's path and stat metadata, plus one column per **`{name}` capture**
in the table's glob.

Facts are **opt-in by DDL**: only facts whose name appears as a column in
the table's `CREATE TABLE` are populated; the rest are silently dropped.
Declaring them requires nothing else.

## Virtual columns

| Column | Type | Value |
|---|---|---|
| `_path` | TEXT | The file's path relative to the scan root (e.g. `posts/hello.md`). |
| `_basename` | TEXT | The filename, including extension (`hello.md`). |
| `_dir` | TEXT | The parent directory relative to the root (`posts`); the empty string for files directly under the root. |
| `_ext` | TEXT | The file extension without the leading dot (`md`). Original case is preserved — `Photo.JPG` yields `JPG`; use `LOWER(_ext)` for case-insensitive matching. `NULL` when the file has no extension. |
| `_size` | INTEGER | File size in bytes. |
| `_mtime` | INTEGER | Last-modified time, Unix seconds. |
| `_ctime` | INTEGER | Creation (birth) time, Unix seconds. `NULL` when the platform or filesystem cannot supply it. |

A fact that cannot be computed (an unreadable file's `_size`/`_mtime`/
`_ctime`, a missing extension's `_ext`) is absent from the row: `NULL` in
the default relaxed mode, a missing-column error for a
[`strict`](./config.md#table) table that declares it.

```sql
SELECT _basename, _size
FROM posts
WHERE _mtime > strftime('%s', '2024-01-01')
ORDER BY _mtime DESC;
```

## Glob captures

A `{name}` segment in a table's glob captures part of each matched path as
a TEXT column named `name`:

```toml
[[table]]
ddl  = "CREATE TABLE comments (thread_id TEXT, _basename TEXT, _mtime INTEGER)"
glob = "_comments/{thread_id}/*.jsonl"
```

A file at `_comments/abc123/2024-05-05.jsonl` produces a row with
`thread_id = "abc123"`.

- A capture name must be a valid identifier: a letter or underscore
  followed by letters, digits, or underscores (`[a-zA-Z_][a-zA-Z0-9_]*`).
- A capture matches **within a single path segment** — one or more
  characters, never a `/`. For matching purposes, `{name}` behaves like
  `*`.
- A glob may contain multiple captures (`{year}/{month}/*.jpg`).
- Like virtual columns, a capture populates a column only when the DDL
  declares a column of the same name.

## Precedence

Values produced by a table's own row source — an `on-file` command's JSON
output or an SDK `extract` callback's return value — **win** over
auto-injected facts of the same name. An extract that explicitly emits
`_path` is honored.

Injection order per row: virtual columns first, then glob captures, then
the row source's own values, each layer overwriting the previous, all
filtered to the DDL's declared columns.
