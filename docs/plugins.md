# Plugins

A **plugin** is an ordinary Python package that ships a `dirsql.toml` config
fragment and declares itself via a `dirsql` entry point. Installing it in the
same environment as `dirsql` activates it: the `pip`/`uvx` launcher discovers
the package and loads its fragment automatically, with zero config edits. The
full discovery contract (ordering, opt-out, failure modes) is in the
[CLI reference](./reference/cli.md#plugins); to build your own, see
[Write a plugin](./howto/write-a-plugin.md).

This page lists the first-party plugins.

## `dirsql-plugin-embeddings`

Semantic search over files. The plugin's product is content → vectors: it
declares an `embed()` SQL scalar function (via
[`[[dirsql.function]]`](./reference/config.md#dirsql-function)) that turns
TEXT or BLOB values into embedding vectors, and loads
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) so
`vec_distance_cosine()` and friends do the distance math. You scope the
search with an ordinary [path-table](./reference/path-tables.md) glob, rank
with `ORDER BY`, and cut with `LIMIT` — search is plain SQL.

[PyPI](https://pypi.org/project/dirsql-plugin-embeddings/) ·
[Source](https://github.com/thekevinscott/dirsql/tree/main/plugins/dirsql-plugin-embeddings)

### Install and launch

The plugin is a normal PyPI package; installing it alongside `dirsql` is the
whole install story (installed = active — there is no enable step, and no
configuration at all):

```sh
uvx --with dirsql-plugin-embeddings dirsql "
  SELECT path
  FROM (SELECT path, embed(content ->> 'abstract') AS emb
        FROM './arxiv-firehose/data/**/metadata.json')
  WHERE emb IS NOT NULL
  ORDER BY vec_distance_cosine(emb, embed('local private models'))
  LIMIT 10"
```

The launcher finds the package through its `dirsql` entry point and injects
the shipped `dirsql.toml` fragment as an ordinary `-c` flag, composed after
your own configs. The fragment declares the `sqlite-vec` extension (resolved
from the installed `sqlite-vec` package, which the plugin depends on) and the
`embed()` function entry. Discovery can be turned off per-invocation with
`--no-plugin` or `DIRSQL_NO_PLUGIN=1`
([reference](./reference/cli.md#plugins)).

For the common case — one glob, one question, top-k paths — the package is
also its own command, generating and running exactly that SQL:

```sh
uvx dirsql-plugin-embeddings '**/*.md' "local private models" -k 10
```

The corpus glob is a **required** first positional — the plugin never picks a
default corpus for you — and a bare glob is normalized to the `./`-relative
form the SQL layer requires. Results print as ranked `path<TAB>distance`
lines, closest first. See
[Search documents by meaning](./howto/search-by-meaning.md) for the guide to
both styles.

### Zero cost when unused

`embed()` is [inert until called](./reference/config.md#worker-lifecycle):
installing the plugin changes nothing for queries that never call it. No
worker process is spawned, no model is loaded or downloaded, and no cache is
touched. Only what a query's glob actually selects is ever embedded — the
worker receives **values, not paths**, and never opens files itself.

### Model

Embeddings come from [model2vec](https://github.com/MinishLab/model2vec)
static models — inference needs numpy and tokenizers only, no torch, so the
plugin stays light enough for `uvx` ephemeral environments and is fast on
CPU. The default model is
[`minishlab/potion-retrieval-32M`](https://huggingface.co/minishlab/potion-retrieval-32M).

The very first `embed()` call downloads the model (on the order of a hundred
megabytes — expect seconds to a few minutes depending on your connection,
with progress on stderr) into the standard Hugging Face cache
(`~/.cache/huggingface`), which persists across `uvx` environments; every
later run loads it from disk.

Override the model per call with the optional second argument —
`embed(text, 'model-id')` — or per run with the one-liner's `--model` flag,
which templates the same second argument. The id must be a
**model2vec-loadable** model; sentence-transformers/torch models are out of
scope.

### Vector cache

Computed vectors are cached at `~/.cache/dirsql/embeddings/` (respecting
`XDG_CACHE_HOME`), keyed on the SHA-256 of the value bytes plus the model
identifier. Changing either recomputes — switching models never serves stale
vectors — and re-running a query over unchanged files is cache hits all the
way. There is no eviction: the directory is **safe to wipe at any time**; the
only cost is re-embedding. The cache never lives inside a queried tree — the
worker writes nothing into the directories you query.
