# Path-tables

A **path-table** is a table you never declare. Write a path where a table name
goes, and dirsql scans the filesystem for you:

```sql
SELECT basename, size FROM './' ORDER BY size DESC LIMIT 5;
```

No `.dirsql.toml` entry, no DDL, no `on_file`. The name *is* the query.

## How resolution works

dirsql never parses your SQL. It hands the statement to SQLite untouched; only
when SQLite reports `no such table: X` does dirsql look at `X`:

1. If `X` starts with `./`, dirsql registers a path-table over the matching
   files and re-prepares the statement.
2. Otherwise the SQLite error stands, unchanged.

Because discovery rides on SQLite's own errors, joins, subqueries and CTEs work
with no extra machinery — SQLite names each missing target in turn, and dirsql
resolves them one at a time.

Two consequences follow directly:

- **A declared table always wins.** The fallback only runs after SQLite has
  already failed to find the name, so a real table is found first. A table you
  genuinely named `"./"` shadows the path, and dirsql will not argue.
- **A typo stays a typo.** `SELECT * FROM usrs` fails with SQLite's error and
  nothing else. dirsql never guesses that an ordinary identifier meant a file.

## Writing the path

A `./` path is relative to the **index root** — the directory dirsql is
indexing, not your shell's working directory.

**Directories are recursive by default.** Naming a directory scans everything
beneath it; the non-recursive form is spelled explicitly with `*`.

| You write | dirsql scans |
| --- | --- |
| `'./'` | every file under the index root, recursively |
| `'./docs'` | every file under `docs/`, recursively |
| `'./*'` | files directly inside the index root, and no deeper |
| `'./docs/*.md'` | markdown files directly inside `docs/` |
| `'./docs/**/*.md'` | markdown files at any depth under `docs/` |
| `'./notes/today.md'` | exactly that one file — one file is one row |

A path containing `*`, `?` or `[` is a glob and is used exactly as written: `*`
matches within a single directory, `**` crosses directories.

A path naming a single file yields exactly one row. dirsql never splits a file
into rows on its own — that is what a table's `on_file` hook is for.

The `./` is required for index-relative paths. A bare glob is rejected with a
hint rather than silently accepted:

```
SELECT * FROM '**/*.md';
-- no such table: **/*.md; did you mean './**/*.md'?
```

**The quotes are required too.** A path is not a bare SQL identifier — `./` is
punctuation to SQLite's parser, so an unquoted path fails at parse time, before
dirsql ever sees a table name to resolve. Single or double quotes both work;
the error names the quoted form:

```
SELECT * FROM ./;
-- near ".": syntax error in SELECT * FROM ./ at offset 14
-- hint: paths used as table names must be quoted; did you mean "./"?
```

### Paths outside the index root

Three other prefixes resolve, with their usual shell meanings:

| You write | dirsql scans |
| --- | --- |
| `'/var/log/*.log'` | an absolute path |
| `'../notes'` | relative to the index root's parent |
| `'~/notes/*.md'` | relative to your home directory |

`..` is folded out textually, not followed through symlinks, so the directory
scanned is a function of the string you wrote.

**These report absolute `path` values.** A `./` path-table reports paths
relative to the index root, matching every other dirsql table; a `/`, `../` or
`~/` path-table has no meaningful relative base — the root it scans is derived
from the pattern, not named by you — so it reports the full path instead. The
value you get back is one you can paste into another command:

```sql
SELECT path FROM '/var/log/*.log';
-- /var/log/syslog
```

On a system with no home directory, a `~/` path-table reports that it cannot
resolve rather than guessing.

## Columns

A path-table has the same [virtual columns](/reference/columns) as any dirsql
table:

| Column | Type | Meaning |
| --- | --- | --- |
| `path` | TEXT | path relative to the index root (absolute for `/`, `../`, `~/` tables) |
| `basename` | TEXT | filename with extension |
| `dir` | TEXT | parent directory, relative to the index root |
| `ext` | TEXT | extension without the dot |
| `size` | INTEGER | size in bytes |
| `mtime` | INTEGER | modification time, Unix seconds |
| `ctime` | INTEGER | creation/change time, Unix seconds |

There is also a hidden `content` column holding the file's text. It is excluded
from `SELECT *` and read only when you name it, so scanning a large tree costs
nothing until you ask for file bodies:

```sql
SELECT path FROM './docs/*.md' WHERE content LIKE '%deprecated%';
```

A file that cannot be read, or is not valid UTF-8, yields `NULL` content rather
than failing the query.

## Parsing rows with `--on-file`

By default a path-table's columns are the stat columns above — one row per
file. When you want *one row per record inside* each file, attach a parser with
the `dirsql query` flag [`--on-file`](/reference/cli):

```sh
dirsql query "SELECT title, author FROM './posts/*.md'" \
  --on-file 'extract.py {path}'
```

The command runs once per matched file and prints a JSON array of row objects,
exactly like a declared table's [`on-file` hook](/reference/hooks) — same argv
splitting, same `{path}`/`{root}` placeholders, same timeout. Its output *is*
the table:

- **The parser supplies the whole schema.** Columns are inferred from the keys
  across the emitted rows. The stat columns (`path`, `size`, …) are **not**
  reachable on a parsed path-table — a parser that wants the path emits it (it
  has `{path}`). The two modes stay cleanly separate.
- **Failures are isolated per file.** A file whose parser fails (spawn, non-zero
  exit, timeout, or no output) or whose output is not a JSON array of rows
  contributes no rows; a one-line warning naming the file goes to stderr and the
  scan continues. The schema is inferred from the files that did parse.
- **The skip rules still apply.** A parsed scan honors the same `node_modules`
  /`.git`/`ignore` rules a stat scan does (see below).
- **`--persist` skips the parser for unchanged files.** With a
  [persistent cache](/howto/persist), each file's parser output is stored
  against its stat metadata, so a later run over an unchanged tree serves the
  rows from the cache and spawns no process. Change the file, the glob, the
  parser command, or the dirsql version and that file (or that whole table) is
  parsed again.

`--on-file` applies to **every** path-table in the query and may be given **at
most once**. For different parsers per file set, define named tables in a
`.dirsql.toml` with their own `on-file` keys and pass it with `-c` — the flag
never touches config-declared tables. It is a `query`-subcommand flag; server
mode does not accept it.

## Freshness and scope

A path-table is scanned when the statement runs, so it always reflects the
filesystem as it is *now* — unlike declared tables, which are indexed on build
and updated by the watcher. A file created a moment ago shows up immediately.

The scan is live all the way down to `content`: a file's body is read when the
query names the `content` column, not when the row is discovered. A file
deleted *after* the scan finds it but *before* its `content` is read yields
`NULL` content — the same NULL an unreadable or non-UTF-8 file gives — rather
than failing the query. This is an accepted consequence of reading live, not a
bug to design around.

The table itself is per-connection: it lives in `temp`, so it cannot leak into
`sqlite_master` or survive a restart. Under `--persist` a *parsed* table's rows
outlive the connection in the cache (above), but the table is still minted
fresh each run and the scan still decides what exists. The reserved top-level
`.dirsql/` directory is excluded from the scan, as everywhere else.

### When to promote to a declared table

Every query re-scans the filesystem: a path-table has no index and no watcher.
That is the right trade for a hundreds-of-files, run-it-once question. When the
same tree is queried repeatedly, or is large, declare a
[table](/reference/config) for it instead — a declared table is indexed on
build, kept fresh by the watcher, and (with `--persist`) survives restarts, so
its rows are read from SQLite rather than re-walked each time.

## Skip rules

A path-table scan applies the same [`ignore`](/reference/config) patterns your
declared tables use, plus two built-in defaults so a zero-config
`SELECT * FROM './'` does not drown in machinery:

- `**/node_modules/**`
- `**/.git/**`

Both apply at any depth, so a `node_modules` nested inside a subdirectory is
skipped just like one at the top.

### `.gitignore`

Path-table scans also respect `.gitignore` files by default, the way fd and
ripgrep do: a `.gitignore` anywhere in the tree applies below its own
directory, deeper files override shallower ones, `!pattern` re-includes, and
an ignored directory is pruned rather than walked. In a typical repo this
excludes build output, virtualenvs, and caches with zero ceremony. No `.git`
directory is required — a `.gitignore` in any scanned directory counts — and
the built-in defaults above remain as a floor for directories with no
`.gitignore` at all.

Pass [`--no-ignore`](./cli.md#flags) to restore the full walk — the
determinism switch for scripted use, since results otherwise depend on
`.gitignore` state. It disables only the `.gitignore` respect; the built-in
defaults and configured `ignore` patterns still apply.

### Naming a skipped directory

Skip rules are judged on the part of the path *below* what you named outright,
so pointing at a skipped directory — built-in or gitignored — still scans it:

```sql
SELECT path FROM './';                     -- no node_modules rows
SELECT path FROM './node_modules/*/package.json';  -- scans it anyway
SELECT path FROM './dist';                 -- scans dist/ even when gitignored
```

A `.gitignore` at or below the directory you named still filters beneath it;
only rules inherited from above it are set aside.

### Hidden files

Dotfiles are ordinary files: `'./'` and `'./*'` include them, with or without
`--no-ignore`. This is a deliberate divergence from fd/ripgrep — querying
dotfile directories (`.claude/`, …) is a first-class `dirsql` use case. Add an
`ignore` pattern if you would rather not see them.

## Joining against declared tables

Path-tables are ordinary SQLite tables once resolved, so they join freely:

```sql
SELECT p.basename, d.size
FROM './docs/*.md' AS p
JOIN pages AS d ON d.path = p.path;
```

A zero-match path-table is not an error — it is an empty table, and the query
returns no rows.
