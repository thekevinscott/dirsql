---
name: dirsql
description: Answer a question about a directory tree by writing SQL and running it with the dirsql CLI. Use when the answer spans many files - counting, grouping, or aggregating files; "which files are ..." by size, mtime, or extension; pulling the same JSON or frontmatter field out of many files; comparing or joining two directories. Do not use to read one known file (Read), hunt for a literal string (grep), or list a handful of names (ls).
---

# dirsql

The current directory is a database. Nothing to install; run from the directory in question:

```dirsql
uvx dirsql "SELECT basename, size FROM './**' ORDER BY size DESC LIMIT 5" --format json
```

Always pass `--format json`. The default `auto` keys on stdout: a terminal gets a table, a pipe gets JSON, and you cannot know which you are. With it, stdout is one JSON array of row objects, every time. Errors go to stderr with exit `1`.

## Path-tables

A quoted path stands in for a table name. Paths are relative to the directory you run in.

| You write | dirsql scans |
| --- | --- |
| `'./'` | files directly here, no deeper |
| `'./docs'` | files directly inside `docs/`, no deeper |
| `'./**'` | every file, any depth |
| `'./docs/**'` | every file under `docs/`, any depth |
| `'./docs/*.md'` | markdown directly inside `docs/` |
| `'./**/*.md'` | markdown at any depth |
| `'./notes/today.md'` | that one file, one row |

A directory name is one level, like `ls docs`; `*` matches one level and `**` any depth, as in the shell. To scan a whole tree, write `'./**'`. `'/var/log/*.log'`, `'../notes'` and `'~/notes/*.md'` also resolve, and those report absolute `path` values.

## Two things that break

1. **The path must be quoted.** `FROM ./` is a SQLite syntax error; write `FROM './'`.
2. **`content ->> 'key'` takes a string literal.** `content ->> key` fails with `no such column: key`.

## Columns

Every row has `path`, `basename`, `dir`, `ext` (no dot, `NULL` when absent), `size` (bytes), `mtime` and `ctime` (Unix seconds). A hidden `content` column holds the file's text: excluded from `SELECT *`, read only when you name it, `NULL` for unreadable or non-UTF-8 files.

## What a scan skips

`.gitignore` is respected by default, and `node_modules` and `.git` are always skipped. Dotfiles are included. Pass `--no-ignore` to scan gitignored files, or name the directory outright (`'./dist'` scans it even when ignored).

## Escalation

1. A path-table, as above. Fits a question you ask once.
2. The same tree queried repeatedly, or a per-record parser you want to keep: declare a table in a `.dirsql.toml` and pass it with `-c .dirsql.toml`. [Config reference](https://dirsql.dev/reference/config)
3. One row per record inside each file, inline: `--on-file <command>` attaches a parser to every path-table in the query; its output replaces the stat columns. [Hook contract](https://dirsql.dev/reference/hooks)
4. Search by meaning: `uvx --with dirsql-plugin-embeddings dirsql "<sql>" --format json` adds `embed()` and `vec_distance_cosine()`. [Search by meaning](https://dirsql.dev/howto/search-by-meaning)

## Read-only

dirsql never writes to, moves, or changes the files it scans, and mutating SQL is rejected. Never pass `--persist`.

## More

- Verified queries: [references/recipes.md](references/recipes.md)
- Error string to fix: [references/troubleshooting.md](references/troubleshooting.md)
- Full docs: [dirsql.dev](https://dirsql.dev/) ([path-tables](https://dirsql.dev/reference/path-tables), [columns](https://dirsql.dev/reference/columns), [CLI](https://dirsql.dev/reference/cli), [query JSON](https://dirsql.dev/howto/query-json)); source under `docs/` in the dirsql repo.
