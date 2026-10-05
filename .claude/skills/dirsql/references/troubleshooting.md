# Troubleshooting

Diagnostics are on stderr; the exit code is `1`.

## `no such table: **/*.md; did you mean './**/*.md'?`

A bare glob. Index-relative paths start with `./`:

```dirsql
uvx dirsql "SELECT path FROM './**/*.md'" --format json
```

## `near ".": syntax error` ... `hint: paths used as table names must be quoted`

`FROM ./` reaches SQLite's parser as punctuation. Quote the path:

```dirsql
uvx dirsql "SELECT COUNT(*) AS files FROM './'" --format json
```

## `no such column: disabled`

`content ->> disabled` names a column. The key is a string literal:

```dirsql
uvx dirsql "SELECT path, content ->> 'disabled' AS disabled FROM './**/metadata.json' WHERE json_valid(content)" --format json
```

## `SQLite error: malformed JSON`

One file in the glob is not valid JSON (an empty file counts), and that fails the whole query. Guard with `json_valid`:

```dirsql
uvx dirsql "SELECT path, content ->> 'name' AS name FROM './**/*.json' WHERE json_valid(content)" --format json
```

## `no such table: files; did you mean FROM './**'?`

There is no implicit table. Query the tree directly:

```dirsql
uvx dirsql "SELECT path FROM './**'" --format json
```

## Empty `[]` for files you can see

- The files are gitignored. `.gitignore` is respected by default; `--no-ignore` scans them:

  ```dirsql
  uvx dirsql "SELECT path FROM './**'" --format json --no-ignore
  ```

  Naming the directory outright also works: `'./build'` scans `build/` even when ignored.
- The directory is `node_modules` or `.git`. Those are always skipped unless named outright.
- You named a directory (`'./'`, `'./docs'`), which lists one level only. `'./**'` and `'./docs/**'` are the recursive forms.

## `table ./ may not be modified`

dirsql is read-only. Mutations belong in a script that consumes the query's JSON.

## A table appeared, not JSON

`--format json` was not passed and stdout was a terminal. Pass it.
