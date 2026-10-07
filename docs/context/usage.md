## Usage

dirsql answers a question about a directory tree with SQL. Nothing to install; run it from the directory in question:

```sh
uvx dirsql "SELECT basename, size FROM './**' ORDER BY size DESC LIMIT 5" --format json
```

Always pass `--format json`. Without it the output depends on stdout: a terminal gets a table, a pipe gets JSON, and you cannot know which you have. With it, stdout is one JSON array of row objects. Errors go to stderr with exit code `1`.

### Path-tables

A quoted path stands in for a table name. Paths are relative to the directory you run in.

| You write | dirsql scans |
| --- | --- |
| `'./'` | files directly here, no deeper |
| `'./docs'` | files directly inside `docs/`, no deeper |
| `'./*.md'` | markdown directly here |
| `'./**'` | every file, any depth |
| `'./docs/**'` | every file under `docs/`, any depth |
| `'./**/*.md'` | markdown at any depth |
| `'./notes/today.md'` | that one file |

`*` matches within one directory level and `**` matches any depth. A directory name lists one level, like `ls docs`, so write `'./**'` to scan the whole tree. Rows are files only, never directories. `'/var/log/*.log'`, `'../notes'` and `'~/notes/*.md'` also resolve; `path` is reported as written, so `'../notes'` gives `../notes/x.md` and the others give absolute paths.

Two mistakes fail every time:

1. **The path must be quoted.** `FROM ./` is a syntax error; write `FROM './'`.
2. **`content ->> 'key'` takes a string literal.** `content ->> key` fails with `no such column: key`.

### Columns

Every row has `path`, `basename`, `dir`, `ext` (no dot, `NULL` when absent), `size` (bytes), `mtime` and `ctime` (Unix seconds). A hidden `content` column holds the file's text: it is left out of `SELECT *`, read only when you name it, and `NULL` for unreadable or non-UTF-8 files.

### What a scan skips

- **`.gitignore` is respected inside a git repo**, as git does; outside one (no enclosing `.git`) no `.gitignore` applies. Pass `--no-ignore` to scan gitignored files too, or name the ignored directory outright: `'./build'` scans `build/`.
- **`'./**'` does not enter `.git`.** Naming it scans it: `'./.git/**'`.
- **To match dotfiles, spell the dot:** `'./.github/**'`, `'./**/.env'`.

### Escalation

1. A path-table, as above. Fits a question you ask once.
2. The same tree queried repeatedly, or a parser you want to keep: declare a table in a `.dirsql.toml` and pass it with `-c .dirsql.toml`. See [config](https://dirsql.dev/reference/config).
3. One row per record inside each file, inline: `--on-file <command>` attaches a parser to every path-table in the query. The parser prints one JSON object per line (NDJSON, never an array), and its output replaces the stat columns. See [hooks](https://dirsql.dev/reference/hooks).
4. Search by meaning: `uvx --with dirsql-plugin-embeddings dirsql "<sql>" --format json` adds `embed()` and `vec_distance_cosine()`. See [search by meaning](https://dirsql.dev/howto/search-by-meaning).

### Read-only

dirsql never writes to, moves or changes the files it scans, and it rejects mutating SQL. Do not pass `--persist`: it writes a cache database into the directory.

Full docs: [dirsql.dev](https://dirsql.dev/) ([path-tables](https://dirsql.dev/reference/path-tables), [columns](https://dirsql.dev/reference/columns), [CLI](https://dirsql.dev/reference/cli), [query JSON](https://dirsql.dev/howto/query-json)).
