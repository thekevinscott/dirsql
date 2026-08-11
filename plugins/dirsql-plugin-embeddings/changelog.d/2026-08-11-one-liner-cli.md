**Added** the one-liner search CLI as the default command:
`dirsql-plugin-embeddings '<glob>' '<query>' [-k/--limit N] [--model ID]`.
The corpus glob is a required first positional (no default corpus), the query
text the second; `-k`/`--limit` (default 10) is exactly the SQL `LIMIT`, and
`--model` templates the model id as `embed()`'s second argument in the
generated SQL. The command builds the canonical search SQL (an `embed()`
subquery over the glob's rows, `vec_distance_cosine` against
`embed('<query>')`, `ORDER BY distance LIMIT k`), runs it via the dirsql
Python SDK (now a declared dependency; needs dirsql >= 0.4.17, the first
release with `[[dirsql.function]]`) with the packaged
config fragment, and prints ranked `path<TAB>distance` lines. Query text,
model id, and glob are SQL-escaped. The `worker` subcommand is unchanged;
there is no explicit `search` spelling — a literal `search` first token is a
corpus glob.
