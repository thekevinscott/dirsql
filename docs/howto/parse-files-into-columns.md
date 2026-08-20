# Parse your files into columns

A [path-table](../reference/path-tables.md) gives you one row per file, with the
stat columns (`path`, `size`, `mtime`, …). When the columns you actually want
live *inside* each file, attach a parser with
[`--on-file`](../reference/cli.md#on-file-command): prototype it inline on the
path-table, then paste the **same** command into a config file the day you want
a watcher, persistence, or more than one table. The parser command never
changes across that move — that is the whole point.

## 1. Start from a bare path-table

You have a directory of Markdown posts with YAML frontmatter:

```
posts/hello-world.md
posts/on-recursion.md
```

A path-table already answers questions about the files themselves:

```bash
dirsql query "SELECT basename, size FROM './posts/*.md' ORDER BY basename"
```

```json
[{"basename":"hello-world.md","size":65},{"basename":"on-recursion.md","size":102}]
```

But `title` and `author` are inside the frontmatter, not in the stat columns.
To get them, you need a parser.

## 2. Attach a parser with `--on-file`

Any program that reads one file and prints a **JSON array of row objects** on
stdout is a parser. Here is a small one, `extract.py`, that reads a post's
frontmatter:

```python
#!/usr/bin/env python3
import json, re, sys

text = open(sys.argv[1], encoding="utf-8").read()
m = re.match(r"^---\n(.*?)\n---", text, re.DOTALL)
fields = dict(
    (k.strip(), v.strip())
    for k, _, v in (line.partition(":") for line in (m.group(1).splitlines() if m else []))
)
print(json.dumps([{"title": fields.get("title"), "author": fields.get("author")}]))
```

Point the path-table at it with `--on-file`:

```bash
dirsql query "SELECT title, author FROM './posts/*.md' ORDER BY title" \
  --on-file 'python3 extract.py {path}'
```

```json
[{"author":"Ada Lovelace","title":"Hello World"},{"author":"Alan Turing","title":"On Recursion"}]
```

Now the parser's output *is* the table. `--on-file` runs the command once per
matched file; `{path}` is the file's absolute path, one of the placeholders in
the shared [`on-file` hook contract](../reference/hooks.md#on-file) (argv
splitting, timeout, and per-file failure isolation all come from there). The
stat columns are no longer reachable — a parser that wants the path emits it,
since it already has `{path}`. See
[Parsing rows with `--on-file`](../reference/path-tables.md#parsing-rows-with-on-file)
for the full behavior.

`--on-file` applies to every path-table in the query and may be given at most
once. It is a `query`-only flag: there is no config file involved yet, so it is
the fastest way to see whether a parser produces the rows you expect.

## 3. Graduate to a config file

The inline flag re-scans and re-parses every file on every query, defines one
parser for the whole query, and forgets everything when the process exits. When
you want a **watcher** that keeps the rows fresh, **persistence** across
restarts, or **different parsers for different file sets**, move the parser into
a `.dirsql.toml` [`[[table]]`](../reference/config.md#table) — and paste the
command in verbatim:

```toml
[[table]]
name = "posts"
ddl     = "CREATE TABLE posts (title TEXT, author TEXT)"
glob    = "posts/*.md"
on-file = "python3 extract.py {path}"
```

The `on-file` value is byte-for-byte the string you passed to `--on-file`. The
[`on-file` hook contract](../reference/hooks.md#on-file) is the same contract
the flag used — the flag and the config key are two spellings of the same
attachment. What you gain by moving to config is everything around the parser:

```bash
dirsql query "SELECT title, author FROM posts ORDER BY title" -c ./.dirsql.toml
```

```json
[{"author":"Ada Lovelace","title":"Hello World"},{"author":"Alan Turing","title":"On Recursion"}]
```

Same command, same rows. The difference is what a declared table brings: it is
indexed on build, kept fresh by the watcher, survives restarts with
[`--persist`](./persist.md), and a config can declare
[many tables](./define-tables.md) — each with its own `on-file` — where the flag
gives every path-table one parser. In both spellings the table's columns are
exactly what the parser emits: `dirsql` merges no filesystem facts back on. A
row that needs the file's `path` emits it (the parser has `{path}`).

## Going further

- The full stat-vs-parsed behavior, failure isolation, and skip rules:
  [Parsing rows with `--on-file`](../reference/path-tables.md#parsing-rows-with-on-file).
- Starting from a declared table instead of a path-table?
  [Extract rows from file contents](./extract-from-contents.md) covers the
  config-first path.
- One row per record *within* a file (JSONL, multiple frontmatter blocks): the
  parser prints one object per row — the array length is the row count.
