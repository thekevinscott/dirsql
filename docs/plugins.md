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

Semantic search over a directory of documents. The plugin embeds every matched
file — `**/*.{md,markdown,mdx,rst,txt,pdf}` — into a `documents` table through
any OpenAI-compatible `/v1/embeddings` endpoint, then turns each incoming
question into nearest-neighbor SQL over that table, ranked by
[`sqlite-vec`](https://github.com/asg017/sqlite-vec)'s
`vec_distance_cosine()`.

[PyPI](https://pypi.org/project/dirsql-plugin-embeddings/) ·
[Source](https://github.com/thekevinscott/dirsql/tree/main/plugins/dirsql-plugin-embeddings)

It is the packaged form of the
[Search documents by meaning](./howto/search-by-meaning.md) how-to, with that
guide's local `model2vec` model swapped for a hosted endpoint.

### Install and launch

The plugin is a normal PyPI package; installing it alongside `dirsql` is the
whole install story (installed = active — there is no enable step). Point the
three environment variables at any OpenAI-compatible inference server, hosted
or self-managed:

```sh
export DIRSQL_EMBEDDINGS_BASE_URL="https://api.openai.com"
export DIRSQL_EMBEDDINGS_MODEL="text-embedding-3-small"
export DIRSQL_EMBEDDINGS_API_KEY="sk-…"

uvx --with dirsql-plugin-embeddings --with sqlite-vec dirsql server
```

The launcher finds the package through its `dirsql` entry point and injects
the shipped `dirsql.toml` fragment as an ordinary `-c` flag, composed after
your own configs. The fragment declares three things:

- the `sqlite-vec` extension (resolved from the installed `sqlite-vec`
  package — hence the second `--with`), for `vec_distance_cosine()`;
- the `documents` table (`path`, `text`, `embedding`), whose
  [`on-file`](./reference/hooks.md#on-file) hook embeds each matched file at
  index time;
- a [`pre-query`](./reference/hooks.md#pre-query) hook that embeds each
  incoming question and emits the nearest-neighbor SQL, plus a 300-second
  [`hook-timeout`](./reference/config.md#dirsql-keys) so slow embedding calls
  don't hit the 30-second default.

Ask a question as a `{"q": …}` body — via `POST /query` against the server,
or in one shot from the shell:

```sh
uvx --with dirsql-plugin-embeddings --with sqlite-vec \
  dirsql query '{"q": "how do I cook spaghetti?"}'
# [{"path":"pasta.md","distance":0.113}, …]
```

The result is the three nearest documents, closest first. Discovery can be
turned off per-invocation with `--no-plugin` or `DIRSQL_NO_PLUGIN=1`
([reference](./reference/cli.md#plugins)).

### Configuration

The v0.1 configuration surface is environment variables. The hooks run as
subprocesses, so the variables must be set in the environment `dirsql` itself
runs in — they are inherited, not read from a file.

| Variable | Required | Meaning |
|---|---|---|
| `DIRSQL_EMBEDDINGS_BASE_URL` | yes | Base URL of the embeddings server; `/v1/embeddings` is appended. |
| `DIRSQL_EMBEDDINGS_MODEL` | yes | Model name sent in each request. |
| `DIRSQL_EMBEDDINGS_API_KEY` | yes | Bearer token for the `Authorization` header. |
| `DIRSQL_EMBEDDINGS_CACHE_READ` | no | Set to `0` to bypass reads of the on-disk PDF text-extraction cache (see below). Anything else, or unset, leaves the cache on. |

PDF text extraction is the expensive step of a scan, and a scan re-reads every
matched file — so extracted text is cached on disk at
`~/.cache/dirsql-plugin-embeddings/`, keyed on the file's path and mtime. An
edited PDF is a cache miss, never a stale hit. `DIRSQL_EMBEDDINGS_CACHE_READ=0`
forces every call to re-extract; writes still happen, so the cache stays warm
for the next run that reads it.

### What gets indexed

Every file matching `**/*.{md,markdown,mdx,rst,txt,pdf}` under the root.
Everything except `.pdf` is read as UTF-8 text; a `.pdf` is read with
[pypdf](https://pypdf.readthedocs.io), its per-page text joined and embedded
like any other document. The extension check is case-insensitive (`.PDF` is a
PDF), but the glob itself is not — an uppercase-suffixed file is not matched
at all.

The glob is an allowlist rather than `**/*` on cost, not correctness: every
matched file costs a hook subprocess, and every file the plugin can decode
costs a billed embedding call. Widening the list trades money for recall.

A *scanned*, image-only PDF is not an error: pypdf yields no text and the file
is indexed with an empty `text`, exactly like an empty `.md`.

### When a file fails

A file the plugin cannot process — an unreadable file, a failed embedding
call — is skipped, not fatal. Per the
[`on-file` failure contract](./reference/hooks.md#failure-semantics), the file
contributes no rows, `dirsql` names it on stderr and keeps indexing the rest,
and the run exits `23`: a partial index, distinct from `0` (clean) and `1`
(the run failed). From the SDK the same information is available via
`scan_failures()` / `scanFailures()`.

### Planned configurability

v0.1 is deliberately minimal: one provider shape, one table, one file = one
row = one embedding (no chunking), and a hardcoded glob. Configurability —
alternate embedding backends, chunking strategies, choosing which files get
indexed, cache knobs — is tracked in
[#619](https://github.com/thekevinscott/dirsql/issues/619).
