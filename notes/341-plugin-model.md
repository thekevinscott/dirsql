# A lightweight plugin model (#341)

Design proposal for [#341](https://github.com/thekevinscott/dirsql/issues/341).

## Summary

A plugin is **an ordinary ecosystem package** meant to be installable via `uvx` or `npx`:

```bash
uvx --with dirsql-plugin-embeddings dirsql
# or
npx -y --package @dirsql/embeddings dirsql
```

## Anatomy of a plugin

A pip package `dirsql-plugin-embeddings` looks like the following:

```
dirsql-plugin-embeddings/
  pyproject.toml     # deps: sentence-transformers, …  entry point: dirsql-embeddings
  main.py            # on-file / pre-query subcommands
  README.md          # the .dirsql.toml snippet to paste
```

Plugins are automatically loaded when present. 

A plugin's TOML can look like:

```toml
[[table]]
ddl     = "CREATE TABLE embeddings (_path TEXT, chunk TEXT, embedding TEXT)"
glob    = "**/*.md"
on-file = "uv run python dirsql-embeddings on-file {path}"

[dirsql]
pre-query = "uv run python dirsql-embeddings pre-query {args}"
```

## The motivating case, end to end

With the two MVP additions, the plugin's snippet is: one `[[table]]` with
`on-file` + `timeout`, the `[[dirsql.extension]]` entry for sqlite-vec
(existing feature, package-name resolution already works via pip/npm),
`setup-sql` for the vec0 index + triggers, and `pre-query` translating a
natural-language body into a `MATCH`-based KNN query. At small scale the
plugin can skip the extension and `setup-sql` entirely and brute-force cosine
in SQL or in the hook.
