# dirsql-plugin-embeddings

A first-party [`dirsql`](https://github.com/thekevinscott/dirsql) plugin that
adds **semantic search** over a directory of documents -- Markdown, plain
text, reStructuredText and PDFs. It is the worked
implementation behind the [Search documents by
meaning](https://thekevinscott.github.io/dirsql/howto/search-by-meaning) how-to,
swapping that guide's local `model2vec` model for any OpenAI-compatible
`/v1/embeddings` endpoint.

```sh
uvx --with dirsql-plugin-embeddings dirsql
```

Deliberately minimal (v0.1): one embedding provider shape, one table, no
chunking, no config surface beyond three environment variables.

## How it works

The plugin ships a `dirsql.toml` fragment that dirsql discovers when the package
is installed alongside it. The fragment declares:

- the [`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension, for
  `vec_distance_cosine()`;
- a `documents` table whose `on-file` hook embeds each
  `**/*.{md,markdown,mdx,rst,txt,pdf}` file into a TEXT `embedding` column;
- a `pre-query` hook that embeds the incoming question and emits the
  nearest-neighbor SQL.

Both hooks are console scripts that call the same embedder.

Every matched extension except `.pdf` is read as UTF-8 text; a `.pdf` is read with
[pypdf](https://pypdf.readthedocs.io), whose per-page extracted text is joined
and embedded like any other document. The extension check is case-insensitive
(`.PDF` is a PDF), though the glob above is not — globset matches case-sensitively,
so an uppercase-suffixed file needs its own `glob` entry to be picked up at all.

The glob is an allowlist rather than `**/*` on cost, not correctness. Every
matched file costs a hook subprocess, and every file the plugin can decode costs
a billed embedding call — so pointing `**/*` at a tree containing `node_modules`
or `.git` makes for a slow and expensive scan. The list is what is worth
embedding; widening it trades money for recall.

A file the plugin cannot read is skipped, not fatal. The hook exits non-zero,
dirsql names the file on stderr and carries on indexing the rest, and the run
exits `23` — "completed, some files skipped". From the SDK the same information
is on `scan_failures()` / `scanFailures()`. A *scanned*, image-only PDF is not a
failure at all: pypdf yields no text, and the file is indexed with an empty
`text`, exactly like an empty `.md`.

## Configuration

The embedder reads three environment variables (point them at any hosted or
self-managed OpenAI-compatible inference server):

| Variable | Meaning |
|---|---|
| `DIRSQL_EMBEDDINGS_BASE_URL` | Base URL; `/v1/embeddings` is appended. |
| `DIRSQL_EMBEDDINGS_MODEL` | Model name sent in the request. |
| `DIRSQL_EMBEDDINGS_API_KEY` | Bearer token for `Authorization`. |

## Console scripts

| Script | Hook | Input | Output |
|---|---|---|---|
| `dirsql-embeddings-on-file` | `on-file` | a file's absolute path (`argv[1]`), text or PDF | one-line JSON row array with `path`, `text`, `embedding` |
| `dirsql-embeddings-pre-query` | `pre-query` | a raw request body (`argv[1]`) | nearest-neighbor SQL over `documents` |

`pre-query` accepts both a verbatim server body (`{"q": ...}`) and the CLI
`query` subcommand's `{"sql": <arg>}` wrapper, so `dirsql query '{"q": ...}'` and
a real `POST /query` both work.

## Tests

Three tiers, per the dirsql testing conventions:

- **unit** (colocated, mocked seams) — `src/dirsql_plugin_embeddings/*_test.py`
- **integration** (`tests/integration/`) — each console script as a real
  subprocess against a local stub `/v1/embeddings` server
- **e2e** (`tests/e2e/`) — the full loop through the real launcher + `dirsql`
  binary + `sqlite-vec`, nothing mocked but the embedding endpoint
