**Removed**

The plugin's entire v0.1 surface: the declared `documents` table (built
eagerly over the working directory on every `dirsql` invocation), the
`on-file` readers (text and PDF), the `pre-query` hook, both console scripts
(`dirsql-embeddings-on-file`, `dirsql-embeddings-pre-query`), and the
`DIRSQL_EMBEDDINGS_*` environment variables. The shipped `dirsql.toml`
fragment now declares only sqlite-vec extension loading, which carries
forward. The plugin is being rebuilt around a plugin-provided `embed()` SQL
function (#800).
