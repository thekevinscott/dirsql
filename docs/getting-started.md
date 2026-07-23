# Your first dirsql database

In this tutorial you will turn a directory of three tiny markdown files into
a SQL database — with a single command, and without writing any code. You
will:

1. Create the directory and files.
2. Query them straight away with zero configuration.
3. Declare your own table to name and reuse a shape, and query it.

It takes about five minutes.

`dirsql` only ever reads your files — it never writes, moves, or changes
them — so it is safe to point at a real directory of your own once you are
done here. See [Read-only by design](./explanation#read-only-by-design).

**You need:** a terminal with [`jq`](https://jqlang.org/), and Node ≥ 20.11
(for `npx`). Every `npx dirsql` step below also has a `uvx` tab that behaves
identically, if you prefer Python tooling
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

## 2. Query your files

You wrote no configuration and no schema. Ask `dirsql` how many files are in
this directory anyway — from inside `my-notes`, run one command:

::: code-group

```bash [npm]
npx dirsql "SELECT COUNT(*) AS files FROM './'"
```

```bash [PyPI]
uvx dirsql "SELECT COUNT(*) AS files FROM './'"
```

:::

The first run downloads the package (`npx` asks for confirmation — answer
`y`; `uvx` prints download progress), then prints the result:

```
[{"files":3}]
```

Three files, three rows. That one command scanned the directory, handed
SQLite one row per file, ran your SQL, and printed the answer as JSON.

There is no named table here — you never declared one. `'./'` is a
[path-table](./reference/path-tables.md): a quoted path written where a table
name goes. `'./'` means everything under the directory you ran the command
in. The path *is* the query.

## 3. Select some columns

The response is always a JSON array of row objects, so from here on we pipe
it through `jq` to pretty-print. Ask for two columns instead of a count:

::: code-group

```bash [npm]
npx dirsql query "SELECT path, size FROM './' ORDER BY path" | jq
```

```bash [PyPI]
uvx dirsql query "SELECT path, size FROM './' ORDER BY path" | jq
```

:::

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

`path` and `size` are two of the built-in file columns `dirsql` collects for
every file — see [stat columns](./reference/columns.md#stat-columns) for the
full list. (The `size` values are byte counts; they match the output above
because you pasted the files exactly.)

You have a working SQL database over your files, and you never left the
command line.

## 4. Declare a table

A declared table fixes a shape once: you give it a name, scope it to exactly
the files you care about, and then query it by name instead of repeating a
path in every question. It is also the on-ramp to everything a path-table
can't do — a named table can be kept live by the watcher, persisted across
restarts, and given a parser that reads inside your files.

Still inside `my-notes`, create a `.dirsql.toml`:

```bash
cat > .dirsql.toml <<'EOF'
[[table]]
ddl  = "CREATE TABLE notes (dir TEXT, basename TEXT, size INTEGER)"
glob = "notes/**/*.md"
EOF
```

Two keys define the table:

- `glob` selects which files feed the table — every `.md` at any depth under
  `notes/`, relative to the directory the config sits in.
- `ddl` is ordinary `CREATE TABLE` SQL naming the columns you want to keep.
  Each is a [stat column](./reference/columns.md#stat-columns) `dirsql`
  computes for every file.

## 5. Query the table

`dirsql` does not auto-load a `.dirsql.toml` from the current directory, so
pass it explicitly with `-c`, **after** the SQL:

::: code-group

```bash [npm]
npx dirsql "SELECT dir, basename, size FROM notes ORDER BY dir, basename" -c .dirsql.toml | jq
```

```bash [PyPI]
uvx dirsql "SELECT dir, basename, size FROM notes ORDER BY dir, basename" -c .dirsql.toml | jq
```

:::

```json
[
  {
    "basename": "ideas.md",
    "dir": "notes/alice",
    "size": 52
  },
  {
    "basename": "welcome.md",
    "dir": "notes/alice",
    "size": 66
  },
  {
    "basename": "reading-list.md",
    "dir": "notes/bob",
    "size": 41
  }
]
```

You queried `FROM notes` by name — no path, no glob to repeat. And because
`dir` is a real SQL column, you can aggregate on it. Count each author's
notes by their folder:

::: code-group

```bash [npm]
npx dirsql query "SELECT dir, COUNT(*) AS notes FROM notes GROUP BY dir ORDER BY dir" -c .dirsql.toml | jq
```

```bash [PyPI]
uvx dirsql query "SELECT dir, COUNT(*) AS notes FROM notes GROUP BY dir ORDER BY dir" -c .dirsql.toml | jq
```

:::

```json
[
  {
    "dir": "notes/alice",
    "notes": 2
  },
  {
    "dir": "notes/bob",
    "notes": 1
  }
]
```

That's the whole loop: files in a directory, an instant query with no
configuration, and a declared table when you want a named shape to reuse.

## Where to go next

- [Query files without a config](./howto/query-without-config.md) — more
  path-table questions you can ask with no setup at all.
- [Define tables for your files](./howto/define-tables.md) — the full
  `[[table]]` recipe: multiple tables, ignore patterns.
- [Extract rows from file contents](./howto/extract-from-contents.md) —
  pull columns out of *inside* your files with an `on-file` parser.
- [CLI](./reference/cli.md) — every flag, plus running `dirsql` as a
  long-lived HTTP server instead of one-shot queries.
- [HTTP API](./reference/http-api.md) — `POST /query`, plus `GET /events`,
  a live stream of row changes as files change.
- [SDK](./reference/sdk.md) — embed `dirsql` in a Python, Rust, or
  TypeScript program instead of running the CLI.
- Why is the database rebuilt from your files on every query? See
  [how `dirsql` thinks](./explanation.md).
```
