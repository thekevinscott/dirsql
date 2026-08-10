# dirsql-plugin-embeddings

A first-party [`dirsql`](https://github.com/thekevinscott/dirsql) plugin for
semantic search over files.

**Rebuild in progress**
([#800](https://github.com/thekevinscott/dirsql/issues/800)): the previous
surface — a declared `documents` table built eagerly over the working
directory, per-file `on-file` embedding hooks, a `pre-query` hook, and
`DIRSQL_EMBEDDINGS_*` endpoint configuration — has been removed. The plugin is
being rebuilt around a plugin-provided `embed()` SQL function that is inert
until a query calls it.

Today, installing the plugin loads the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension (for
`vec_distance_cosine()` and friends) and nothing else.

```sh
uvx --with dirsql-plugin-embeddings dirsql "SELECT vec_distance_cosine('[1, 0]', '[0, 1]') AS d"
```
