## Troubleshooting

Diagnostics go to stderr and the exit code is `1`.

### `no such table: **/*.md; did you mean './**/*.md'?`

A bare glob. Paths relative to the directory you run in start with `./`:

```sh
uvx dirsql "SELECT path FROM './**/*.md'" --format json
```

### `near ".": syntax error` and `hint: paths used as table names must be quoted`

`FROM ./` reaches SQLite's parser as punctuation. Quote the path:

```sh
uvx dirsql "SELECT COUNT(*) AS files FROM './'" --format json
```

### `no such column: disabled`

`content ->> disabled` names a column. The key is a string literal:

```sh
uvx dirsql "SELECT path, content ->> 'disabled' AS disabled FROM './**/metadata.json' WHERE json_valid(content)" --format json
```

### `SQLite error: malformed JSON`

One matched file is not valid JSON (an empty file counts), and that fails the whole query. Guard with `json_valid`:

```sh
uvx dirsql "SELECT path, content ->> 'name' AS name FROM './**/*.json' WHERE json_valid(content)" --format json
```

### `no such table: files; did you mean FROM './**'?`

There is no implicit table. Query the tree directly:

```sh
uvx dirsql "SELECT path FROM './**'" --format json
```

### Empty `[]` for files you can see

- **You named a directory**, which lists one level only. `'./**'` and `'./docs/**'` are the recursive forms.
- **The files are gitignored**, inside a git repo (outside one, no `.gitignore` applies). `--no-ignore` scans them:

  ```sh
  uvx dirsql "SELECT path FROM './**'" --format json --no-ignore
  ```

  Naming the directory outright also works: `'./build'` scans `build/` even when it is ignored.
- **The files are under `node_modules`**, which is skipped unless the path names it: `'./node_modules/**'`.
- **The files start with a dot, or sit under a directory that does.** Spell the dot: `'./.git/**'`, `'./**/.env'`.

### `table ./ may not be modified`

dirsql is read-only. Mutations belong in a script that consumes the query's JSON.

### A table appeared, not JSON

`--format json` was not passed and stdout was a terminal. Pass it.
