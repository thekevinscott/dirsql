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
        FROM 'arxiv-firehose/data/**/metadata.json')
  ORDER BY vec_distance_cosine(emb, embed('local private models'))
  LIMIT 10"
```

`embed()` is inert until a query calls it: no worker process is spawned and no
model is loaded for queries that never use it. On the first call, dirsql
spawns the plugin's worker process (`dirsql-plugin-embeddings worker`), which
serves every call of the invocation over stdin/stdout.

## Model

Embeddings come from [model2vec](https://github.com/MinishLab/model2vec)
(static embeddings — numpy + tokenizers, no torch), defaulting to
[`minishlab/potion-retrieval-32M`](https://huggingface.co/minishlab/potion-retrieval-32M).
The model downloads to the standard Hugging Face cache on the first ever run,
with progress on stderr when stderr is a TTY.

An optional second argument overrides the model per call — the id must be
model2vec-loadable (sentence-transformers/torch models are out of scope):

```sql
SELECT embed('some text', 'minishlab/potion-base-8M')
```

## Vector cache

Computed vectors are cached at `~/.cache/dirsql/embeddings/` (or
`$XDG_CACHE_HOME/dirsql/embeddings/` when `XDG_CACHE_HOME` is set), keyed by
the SHA-256 of the value bytes plus the model identifier — changing either
recomputes; switching models never serves stale vectors. There is no
eviction: **the directory is safe to wipe at any time**; the only cost is
re-embedding. The cache never lives inside a queried tree — the worker
receives values, not paths, and writes nothing anywhere else.
