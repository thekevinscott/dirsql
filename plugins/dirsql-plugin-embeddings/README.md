# dirsql-plugin-embeddings

A first-party [`dirsql`](https://github.com/thekevinscott/dirsql) plugin for
semantic search over files.

Installing the plugin loads the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension (for
`vec_distance_cosine()` and friends) and declares an `embed()` SQL scalar
function that turns TEXT or BLOB values into embedding vectors:

```sh
uvx --with dirsql-plugin-embeddings dirsql "
  SELECT path
  FROM (SELECT path, embed(content ->> 'abstract') AS emb
        FROM './arxiv-firehose/data/**/metadata.json')
  WHERE emb IS NOT NULL
  ORDER BY vec_distance_cosine(emb, embed('local private models'))
  LIMIT 10"
```

`WHERE emb IS NOT NULL` is not optional bookkeeping: a file that is unreadable
or not valid UTF-8 has NULL content, so its distance is NULL, and SQLite sorts
NULLs *first* ascending — without the guard those files take the top slots.

For the common case — one glob, one question, top-k paths — the package is
also its own command, generating and running exactly that SQL:

```sh
uvx dirsql-plugin-embeddings '**/*.md' "local private models" -k 10
```

- **Corpus glob: required first positional.** The plugin never picks a
  default corpus; you always say which files are in scope. A bare glob is
  fine here — the command normalizes it to the `./`-relative form the SQL
  layer requires (`**/*.md` → `./**/*.md`).
- **Query text: second positional.** Query text, model id, and glob are
  SQL-escaped into the generated query.
- **`-k` / `--limit`** (both spellings, default 10): the number of results.
  It is exactly the SQL `LIMIT` of the generated query — no other cutoff
  exists.
- **`--model <id>`**: templates the model id as `embed()`'s second argument
  in the generated SQL (see [Model](#model)).

Results print one `path<TAB>distance` line per match, closest first.

> **Top-k is `LIMIT k`.** sqlite-vec's `MATCH ... AND k = N` idiom belongs to
> its `vec0` virtual table, which `dirsql` does not use. For plain
> expressions, sqlite-vec's own documented pattern is the one above:
> `ORDER BY vec_distance_cosine(...) LIMIT k`.

## Zero cost when unused

`embed()` is inert until a query calls it: no worker process is spawned and
no model is loaded for queries that never use it. On the first call, dirsql
spawns the plugin's worker process (`dirsql-plugin-embeddings worker`), which
serves every call of the invocation over stdin/stdout. Only the values the
query actually selects are embedded — the worker receives values, not paths,
and never opens files itself.

## Model

Embeddings come from [model2vec](https://github.com/MinishLab/model2vec)
(static embeddings — numpy + tokenizers, no torch), defaulting to
[`minishlab/potion-retrieval-32M`](https://huggingface.co/minishlab/potion-retrieval-32M).
The model downloads to the standard Hugging Face cache on the first ever run
(on the order of a hundred megabytes — seconds to a few minutes depending on
your connection), with progress on stderr when stderr is a TTY; every later
run loads it from disk.

An optional second argument overrides the model per call — the id must be
model2vec-loadable (sentence-transformers/torch models are out of scope):

```sql
SELECT embed('some text', 'minishlab/potion-base-8M')
```

The one-liner's `--model` flag templates the same second argument.

## Vector cache

Computed vectors are cached at `~/.cache/dirsql/embeddings/` (or
`$XDG_CACHE_HOME/dirsql/embeddings/` when `XDG_CACHE_HOME` is set), keyed by
the SHA-256 of the value bytes plus the model identifier — changing either
recomputes; switching models never serves stale vectors. There is no
eviction: **the directory is safe to wipe at any time**; the only cost is
re-embedding. The cache never lives inside a queried tree — the worker
receives values, not paths, and writes nothing anywhere else.

## Docs

- [Search documents by meaning](https://thekevinscott.github.io/dirsql/howto/search-by-meaning)
  — the guide to both invocation styles.
- [`[[dirsql.function]]`](https://thekevinscott.github.io/dirsql/reference/config#dirsql-function)
  — the core mechanism `embed()` is built on.
