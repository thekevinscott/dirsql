# Your first dirsql database

In this tutorial you will turn a directory of three tiny markdown files into
a SQL database you can query over HTTP — without writing any code. You will:

1. Create the directory and files.
2. Start `dirsql` with zero configuration and query it with `curl`.
3. Define your own table in a `.dirsql.toml` and query the new shape.

It takes about five minutes.

`dirsql` only ever reads your files — it never writes, moves, or changes
them — so it is safe to point at a real directory of your own once you are
done here. See [Read-only by design](./explanation#read-only-by-design).

**You need:** a terminal with `curl` and [`jq`](https://jqlang.org/), and
Node ≥ 20.11 (for `npx`). Every `npx dirsql` step below also has a `uvx`
tab that behaves identically, if you prefer Python tooling
([`uv`](https://docs.astral.sh/uv/)).

## 1. Create three files

Paste this whole block into your terminal. It makes a working directory
with a subfolder per note author and writes three tiny markdown notes:

```bash
mkdir -p my-notes/notes/alice my-notes/notes/bob
cd my-notes
cat > notes/alice/welcome.md <<'EOF'
# Welcome

Start here. This folder is about to become a database.
EOF
cat > notes/alice/ideas.md <<'EOF'
# Ideas

- query files with SQL
- watch for changes
EOF
cat > notes/bob/reading-list.md <<'EOF'
# Reading list

- The SQLite file format
EOF
```

(Any directory of files works with `dirsql` — the rest of this tutorial
assumes exactly these three so your output matches ours.)

Check that all three files are in place:

```bash
find notes -type f | sort
```

```
notes/alice/ideas.md
notes/alice/welcome.md
notes/bob/reading-list.md
```

## 2. Start the server

From inside `my-notes`, start `dirsql`:

::: code-group

```bash [npm]
npx dirsql
```

```bash [PyPI]
uvx dirsql
```

:::

The first run downloads the package (`npx` asks for confirmation — answer
`y`; `uvx` prints download progress), then the server starts:

```
Running at localhost:7117
```

That one command scanned the directory, built an ephemeral SQLite database
with one row per file, and started an HTTP server. Leave it running and
open a **second terminal** for the next step.

## 3. Query your files

You gave `dirsql` no configuration, so no named tables exist. Query the
filesystem directly with a [path-table](./reference/path-tables.md) — a
quoted path where a table name goes. `'./'` means everything under the
index root. Ask it how many files there are:

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT COUNT(*) AS files FROM \'./\'"}'
```

```
[{"files":3}]
```

Three files, three rows. The response is always a JSON array of row
objects ([HTTP API](./reference/http-api.md)) — from here on we pipe it
through `jq` to pretty-print. Now select some columns:

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT path, size FROM \'./\' ORDER BY path"}' \
  | jq
```

```json
[
  {
    "path": "notes/alice/ideas.md",
    "size": 52
  },
  {
    "path": "notes/alice/welcome.md",
    "size": 66
  },
  {
    "path": "notes/bob/reading-list.md",
    "size": 41
  }
]
```

`path` and `size` are two of the built-in file columns `dirsql` collects
for every file — see [stat columns](./reference/columns.md#stat-columns)
for the full list. (The `size` values are byte counts; they match the
output above because you pasted the files exactly.)

You have a working SQL database over your files. Next, teach it the
structure your folders already encode.

## 4. Define a table

Look at the paths again: `notes/alice/ideas.md`, `notes/bob/reading-list.md`
— the author's name is a directory segment. A config file can capture it as
a real column.

In your second terminal, still inside `my-notes`, create a `.dirsql.toml`:

```bash
cat > .dirsql.toml <<'EOF'
[[table]]
ddl  = "CREATE TABLE notes (author TEXT, basename TEXT, size INTEGER)"
glob = "notes/{author}/*.md"
EOF
```

Two keys define the table:

- `glob` selects which files feed the table, and `{author}` is a
  [glob capture](./reference/columns.md#glob-captures): whatever directory
  name matches that segment becomes the row's `author` value.
- `ddl` is ordinary `CREATE TABLE` SQL naming the columns you want to keep.

## 5. Restart and query the new shape

Config is read at startup, so go back to the **first terminal**, stop the
server with `Ctrl-C`, and start it again — this time pointing `dirsql` at
your config with `-c` (`dirsql` does not auto-load a `.dirsql.toml` from the
current directory; you always pass it explicitly):

::: code-group

```bash [npm]
npx dirsql -c .dirsql.toml
```

```bash [PyPI]
uvx dirsql -c .dirsql.toml
```

:::

```
Running at localhost:7117
```

This time `dirsql` loaded your `.dirsql.toml` and served the `notes` table
you defined. Query it from the second terminal:

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT author, basename, size FROM notes ORDER BY author, basename"}' \
  | jq
```

```json
[
  {
    "basename": "ideas.md",
    "size": 52,
    "author": "alice"
  },
  {
    "basename": "welcome.md",
    "size": 66,
    "author": "alice"
  },
  {
    "basename": "reading-list.md",
    "size": 41,
    "author": "bob"
  }
]
```

Every row now carries an `author` column extracted from its path — no
extraction code, just a glob. And it is a real SQL column, so you can
aggregate on it:

```bash
curl -s http://localhost:7117/query \
  -H 'content-type: application/json' \
  -d '{"sql":"SELECT author, COUNT(*) AS notes FROM notes GROUP BY author"}' \
  | jq
```

```json
[
  {
    "author": "alice",
    "notes": 2
  },
  {
    "author": "bob",
    "notes": 1
  }
]
```

That's the whole loop: files in a directory, a declarative table on top,
SQL over HTTP.

## Where to go next

- [Configuration file](./reference/config.md) — the complete `.dirsql.toml`
  reference: more tables, ignore patterns, persistence, hooks.
- [CLI](./reference/cli.md) — flags like `--port` and `--config`, plus
  `dirsql init`.
- [HTTP API](./reference/http-api.md) — `POST /query` in full, plus
  `GET /events`, a live stream of row changes as files change.
- [SDK](./reference/sdk.md) — embed `dirsql` in a Python, Rust, or
  TypeScript program instead of running the server.
- Why is the database rebuilt from your files on every startup? See
  [how `dirsql` thinks](./explanation.md).
