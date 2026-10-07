# Extract rows from file contents

Paths and stat metadata only get you so far — when the columns you want live
*inside* the files (JSON fields, frontmatter, log lines), add an
[`on-file`](../reference/config.md#table) command: it runs once for the
table, with every matched file as an argument, and the JSON objects it prints,
one per line, become the table's rows.

## 1. Point a command at the files

Suppose each book is a JSON file:

```json
{"title": "Middlemarch", "author": "George Eliot", "year": 1871}
```

Any program that reads the files named by its arguments and prints **one JSON
object per line** on stdout works. With [`jq`](https://jqlang.org/):

```toml
[[table]]
name = "books"
ddl     = "CREATE TABLE books (title TEXT, author TEXT, year INTEGER)"
glob    = "books/*.json"
on-file = "jq -c '{title, author, year}'"
```

The command runs once for the table, with every matched file's absolute path
appended as an argument, and prints one object per line for all of them — the
[command hook contract](../reference/hooks.md#on-file), which also covers
the argv splitting, working directory, and stdout protocol shared by every
hook.

## 2. Query the extracted columns

Pass the config with [`-c`](../reference/cli.md#flags) (`dirsql` does not
auto-load a `.dirsql.toml` from the current directory):

```bash
dirsql query "SELECT title, author, year FROM books ORDER BY year" -c ./.dirsql.toml
```

```json
[{"author":"Charles Dickens","title":"Bleak House","year":1852},{"author":"George Eliot","title":"Middlemarch","year":1871}]
```

The table's columns are exactly what the command emits, narrowed to the DDL —
`dirsql` adds nothing. To include the file's `path`, have the command emit it
(it has the path); dirsql will not merge it in for you.

## Multiple rows per file

Each printed line is one row. A JSONL file is already one row per line, so
project the fields you want:

```toml
[[table]]
name = "events"
ddl     = "CREATE TABLE events (event TEXT, user TEXT)"
glob    = "logs/*.jsonl"
on-file = "jq -c '{event, user}'"
```

```bash
dirsql query "SELECT event, user FROM events" -c ./.dirsql.toml
```

```json
[{"event":"login","user":"alice"},{"event":"logout","user":"alice"},{"event":"login","user":"bob"}]
```

## When the command fails

The command runs once for the whole table, so its failure is the table's: a
non-zero exit, or output that is not one JSON object per line (an array
is rejected) fails the build, and `dirsql query` exits `1` with the command's stderr tail on
stderr and nothing on stdout.

```
dirsql query: failed to load config: table `books`: on-file command failed: command `jq -c '{title, author, year}'` failed (exit 5): jq: error (at /home/me/library/books/broken.json:1): Cannot index string with string "title"
```

A file the command cannot parse is the command's to handle — skip it, or
emit a row with `NULL`s — because dirsql has no per-file view of the rows.
Details in [failure semantics](../reference/hooks.md#failure-semantics); the
JSON to SQLite value mapping is under
[`on-file` row mapping](../reference/config.md#on-file-row-mapping).

## Going further

- Just want to read a field or two out of some JSON files? SQLite's JSON
  operators work directly on a path-table's `content` column, with no config
  and no command — [Query JSON file contents](./query-json.md).
- The command re-runs on every startup and, over all of the table's files, on
  every change under the table's glob. If it is expensive,
  [keep the index across restarts](./persist.md).
- Embedding `dirsql` in a program instead? The SDK's `on_file` callback
  fills the same role in-process — see
  [Embed `dirsql` in your application](./embed.md).
