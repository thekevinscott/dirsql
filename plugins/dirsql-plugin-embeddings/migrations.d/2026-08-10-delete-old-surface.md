### dirsql-plugin-embeddings: old surface removed (declared table, hooks, env vars)

#### Summary

The plugin's v0.1 surface is deleted with no deprecation period ahead of the
`embed()` SQL-function rebuild (#800): installing the plugin no longer
declares a `documents` table, no longer spawns per-file `on-file` embedding
subprocesses, no longer rewrites queries through a `pre-query` hook, and no
longer reads `DIRSQL_EMBEDDINGS_*` environment variables. Only sqlite-vec
extension loading carries forward. Any workflow that queried the `documents`
table with the plugin installed breaks.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `documents` table | `dirsql query '{"q": "..."}'` returned nearest-neighbor rows from an auto-built `documents` table | Removed; no replacement until the #800 rebuild lands (`SELECT ... FROM documents` now fails with `no such table: documents`) |
| Console scripts | `dirsql-embeddings-on-file <path>`, `dirsql-embeddings-pre-query <body>` on PATH | Removed; uninstall any external callers |
| Env vars | `DIRSQL_EMBEDDINGS_BASE_URL` / `DIRSQL_EMBEDDINGS_MODEL` / `DIRSQL_EMBEDDINGS_API_KEY` (and `DIRSQL_EMBEDDINGS_CACHE_READ`) configured the embedding endpoint | Removed; unset them — nothing reads them |
| sqlite-vec loading | `[[dirsql.extension]]` in the shipped fragment | Unchanged — still loaded when the plugin is installed |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- Any `dirsql` invocation with the plugin installed: previously built the
  `documents` table eagerly — one Python subprocess (and, when configured, one
  billed embedding call) per matched file under the working directory, before
  the query ran; now no subprocess is spawned and no table is created — the
  plugin only loads sqlite-vec.

#### Verification

```bash
uvx --with dirsql-plugin-embeddings dirsql query "SELECT vec_distance_cosine('[1, 0]', '[0, 1]') AS d"
# expected: [{"d":1.0}]
uvx --with dirsql-plugin-embeddings dirsql query "SELECT * FROM documents"
# expected: error containing `no such table: documents`
```
