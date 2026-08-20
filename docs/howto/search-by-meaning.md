# Search documents by meaning

Ask a question in plain language and get the closest documents back — even
when they share no keywords with it. Install
[`dirsql-plugin-embeddings`](../plugins.md#dirsql-plugin-embeddings) and
semantic search becomes plain SQL: the plugin's `embed()` function turns
text into vectors, [`sqlite-vec`](https://github.com/asg017/sqlite-vec)'s
`vec_distance_cosine()` measures distance, and `ORDER BY … LIMIT` does the
ranking. No config, no API keys, no services — the model runs locally.

Suppose short notes live in `notes/*.md`:

```
notes/pasta.md      # boiling spaghetti, olive oil, garlic
notes/branches.md   # git feature branches and pull requests
notes/tomatoes.md   # planting tomato seedlings after the last frost
```

## The one-liner

The plugin package is also its own command. Give it a corpus glob and a
question, and it prints the closest paths, ranked by distance:

```bash
uvx dirsql-plugin-embeddings 'notes/*.md' "how do I cook pasta?" -k 3
```

- The **corpus glob is required**, and always first: the plugin never picks a
  default corpus, so you always say exactly which files are in scope. A bare
  glob is fine — the command normalizes it to the `./`-relative form the SQL
  layer requires.
- The question is the second positional.
- `-k` / `--limit` (both spellings, default 10) is the number of results —
  it is exactly the SQL `LIMIT` of the generated query; there is no other
  cutoff.
- `--model <id>` switches the embedding model
  ([model story](../plugins.md#model)).

The first-ever run downloads the model (on the order of a hundred megabytes,
with progress on stderr); after that it loads from the local cache. Results
print one `path<TAB>distance` line per match, closest first.

## The SQL behind it

The one-liner generates and runs ordinary `dirsql` SQL, and you can write it
yourself when you want more than ranked paths — a different projection, a
join, a `WHERE` clause, a subset of a JSON file's content:

```bash
uvx --with dirsql-plugin-embeddings dirsql "
  SELECT path,
         vec_distance_cosine(emb, embed('how do I cook pasta?')) AS distance
  FROM (SELECT path, embed(content) AS emb FROM './notes/*.md')
  WHERE emb IS NOT NULL
  ORDER BY distance
  LIMIT 3"
```

```json
[{"path":"notes/pasta.md","distance":0.315},{"path":"notes/tomatoes.md","distance":0.881},{"path":"notes/branches.md","distance":0.92}]
```

Neither "cook" nor any other keyword needs to appear in `pasta.md` — the
distance ranking is doing the work.

Reading the query inside-out:

1. The subquery scans the [path-table](../reference/path-tables.md)
   `'./notes/*.md'` and embeds each file's
   [`content`](../reference/path-tables.md#columns) — only the files the
   glob matches are ever read or embedded. In hand-written SQL the `./`
   prefix is [required](../reference/path-tables.md#writing-the-path); only
   the one-liner normalizes a bare glob for you.
2. `embed('how do I cook pasta?')` embeds the question once (the function is
   deterministic, so SQLite reuses the value across rows).
3. `WHERE emb IS NOT NULL` drops the files that could not be embedded. A
   file that is unreadable or not valid UTF-8 has
   [`NULL` content](../reference/path-tables.md#columns), so its embedding
   and its distance are NULL too — and SQLite sorts NULLs *first* ascending,
   so without this line the unrankable files take the top-k slots.
4. `vec_distance_cosine(...)` computes cosine distance between the two
   vectors; `ORDER BY distance LIMIT 3` keeps the three nearest.

Structured files compose with SQL's JSON operators — embed one field instead
of the whole file:

```sql
SELECT path
FROM (SELECT path, embed(content ->> 'abstract') AS emb
      FROM './papers/**/metadata.json')
WHERE emb IS NOT NULL
ORDER BY vec_distance_cosine(emb, embed('local private models'))
LIMIT 10
```

::: tip Top-k is `LIMIT k`
If you know `sqlite-vec` you may reach for its `MATCH … AND k = 10` idiom.
That syntax belongs to `sqlite-vec`'s `vec0` virtual table, which the table
above does not use: a `[[table]]`'s own `name` is always a per-file row table.
For plain expressions, `sqlite-vec`'s own documented pattern is exactly what
this guide uses: `ORDER BY vec_distance_cosine(...) LIMIT k`. To get the `vec0`
idiom instead, declare the `vec0` table alongside the row table in the same
[`ddl` batch](../reference/config.md#batch-ddl) and fill it from a trigger.
:::

## Repeat runs are cheap

Computed vectors are cached on disk, keyed on content and model
([vector cache](../plugins.md#vector-cache)) — re-running a search over
unchanged files skips the model entirely and re-embeds only what changed.
And the plugin costs nothing when idle: a query that never calls `embed()`
spawns no worker and loads no model
([zero cost when unused](../plugins.md#zero-cost-when-unused)).

## How `embed()` gets into SQL

The plugin ships a config fragment declaring `embed()` via
[`[[dirsql.function]]`](../reference/config.md#dirsql-function), which the
`uvx`/`pip` launcher [discovers automatically](../reference/cli.md#plugins).
The same mechanism is open to your own configs and plugins — any external
command that speaks the
[worker protocol](../reference/config.md#worker-protocol) can back a SQL
function. To build one, see [Write a plugin](./write-a-plugin.md).
