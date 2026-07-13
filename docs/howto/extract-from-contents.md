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
ddl     = "CREATE TABLE books (title TEXT, author TEXT, year INTEGER, path TEXT)"
glob    = "books/*.json"
on-file = "jq -c '[{title, author, year}]' {path}"
```

`{path}` is the matched file's absolute path — one of the
placeholders defined by the
[command hook contract](../reference/hooks.md#on-file), which also covers
the argv splitting, working directory, stdout protocol, and timeout shared
by every hook.

## 2. Query the extracted columns

```bash
dirsql query "SELECT title, author, year, path FROM books ORDER BY year"
```

```json
[{"path":"books/bleak-house.json","author":"Charles Dickens","title":"Bleak House","year":1852},{"path":"books/middlemarch.json","author":"George Eliot","title":"Middlemarch","year":1871}]
```

Filesystem facts are still merged onto every row — `path` above comes from
`dirsql`, not from `jq`. When the command emits a key that collides with a
fact, the command wins
([precedence](../reference/columns.md#precedence)).

## Multiple rows per file

Each object in the printed array is one row. To turn a JSONL file into one
row per line, slurp it:

```toml
[[table]]
ddl     = "CREATE TABLE events (event TEXT, user TEXT, path TEXT)"
glob    = "logs/*.jsonl"
on-file = "jq -c -s '.' {path}"
```

```bash
dirsql query "SELECT event, user FROM events"
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
