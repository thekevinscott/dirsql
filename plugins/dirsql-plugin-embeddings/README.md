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

The glob is an allowlist rather than `**/*` for a reason worth knowing: a hook
that exits non-zero aborts the entire scan
([dirsql#697](https://github.com/thekevinscott/dirsql/issues/697)), and reading a
PNG as UTF-8 does exactly that. Matching everything would mean one image anywhere
under the root produces no index at all, so the list stays limited to what the
plugin can actually read.

A PDF that cannot be parsed aborts the scan rather than being skipped: the hook
exits non-zero and dirsql reports the path and the pypdf reason. A *scanned*,
image-only PDF is not a failure — pypdf yields no text, and the file is indexed
with an empty `text`, exactly like an empty `.md`.

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
