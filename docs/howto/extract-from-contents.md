# Extract rows from file contents

Paths and stat metadata only get you so far — when the columns you want live
*inside* the files (JSON fields, frontmatter, log lines), add an
[`on-file`](../reference/config.md#table) command: it runs once per matched
file and its stdout becomes the file's rows.

## 1. Point a command at the files

Suppose each book is a JSON file:

```json
{"title": "Middlemarch", "author": "George Eliot", "year": 1871}
```

Any program that reads a file and prints a **JSON array of row objects** on
stdout works. With [`jq`](https://jqlang.org/):

```toml
[[table]]
ddl     = "CREATE TABLE books (title TEXT, author TEXT, year INTEGER)"
glob    = "books/*.json"
on-file = "jq -c '[{title, author, year}]' {path}"
```

`{path}` is the matched file's absolute path — one of the
placeholders defined by the
[command hook contract](../reference/hooks.md#on-file), which also covers
the argv splitting, working directory, stdout protocol, and timeout shared
by every hook.

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
(it has `{path}`); dirsql will not merge it in for you.

## Multiple rows per file

Each object in the printed array is one row. To turn a JSONL file into one
row per line, slurp it:

```toml
[[table]]
ddl     = "CREATE TABLE events (event TEXT, user TEXT)"
glob    = "logs/*.jsonl"
on-file = "jq -c -s '.' {path}"
```

```bash
dirsql query "SELECT event, user FROM events" -c ./.dirsql.toml
```

```json
[{"event":"login","user":"alice"},{"event":"logout","user":"alice"},{"event":"login","user":"bob"}]
```

## When a file fails

A file whose command errors (or prints something that isn't a JSON array of
objects) contributes no rows: `dirsql` warns on stderr and the scan
continues — one bad file never takes down the index. Details in
[failure semantics](../reference/hooks.md#failure-semantics); the JSON to
SQLite value mapping is under
[`on-file` row mapping](../reference/config.md#on-file-row-mapping).

## Going further

- The command re-runs on every startup and on every change to a matched
  file. If it is expensive, [keep the index across restarts](./persist.md).
- The flagship use of `on-file` — computing embeddings — is
  [Search documents by meaning](./search-by-meaning.md).
- Embedding `dirsql` in a program instead? The SDK's `on_file` callback
  fills the same role in-process — see
  [Embed `dirsql` in your application](./embed.md).
