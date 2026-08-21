# Search documents by meaning

Ask a question in plain language and get the closest documents back — even
when they share no keywords with it. Install
[`dirsql-plugin-embeddings`](../plugins.md#dirsql-plugin-embeddings) and
semantic search becomes plain SQL: the plugin's `embed()` function turns text
into vectors and loads [`sqlite-vec`](https://github.com/asg017/sqlite-vec),
which does the distance math. No API keys, no services — the model runs
locally.

Where those vectors live is the one real decision, and there are two answers:

- **A `vec0` index**, declared in config beside the table and filled by a
  trigger as files are ingested. A query then embeds exactly one string — the
  question — and `sqlite-vec` scans the stored vectors in process. This is the
  shape to build on.
- **A path-table subquery**, with no config at all: embed every matched file
  inline, rank, cut. Ideal for a one-off question over a handful of files.

Suppose short notes live in `notes/*.md`:

```
notes/pasta.md      # boiling spaghetti, olive oil, garlic
notes/branches.md   # git feature branches and pull requests
notes/tomatoes.md   # planting tomato seedlings after the last frost
```

## Try it in one line

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

That command generates the **path-table** shape below — zero setup, and it
embeds every matched file on every run.

## Which shape

Both rank by cosine distance and return the same rows. They differ in what
each query costs and what you have to declare:

| | Path-table subquery | `vec0` index |
|---|---|---|
| Setup | none | a `[[table]]` with a `ddl` batch |
| Freshness | always current — the walk is the read | watcher-maintained; survives restarts under [`--persist`](./persist.md) |
| Per query | one `embed()` round trip **per matched file**, plus a walk that reads every file's content | one `embed()` round trip **total**, plus an in-process KNN scan |
| Good for | a one-off question, a small corpus, an ad-hoc glob | a corpus you query repeatedly |

The index only pays off when the table outlives the query — under
`--persist`, or inside a long-running [`dirsql server`](../reference/cli.md).
A one-shot `dirsql query` with an ephemeral index rebuilds the table, and
therefore re-embeds the corpus, before it answers; that is strictly more work
than the subquery.

## Build a vector index

### 1. Find your model's width

`vec0` fixes the vector length when the table is declared, so you need the
number your model returns:

```bash
uvx --with dirsql-plugin-embeddings dirsql "SELECT vec_length(embed('probe')) AS width"
```

Whatever that prints is what goes in `float[…]` below — the examples here
write `512`. Get it wrong and the table load fails with an error naming both
figures (`Expected 512 dimensions but received 768.`), so this is cheap to
correct.

### 2. Declare the table and the index

A named table's rows come from its [`on-file`](../reference/config.md#table)
hook — dirsql injects nothing — so put a small reader next to the config.
`note.py` prints one row per file, carrying the path to display and the text
to embed:

```python
#!/usr/bin/env python3
import json, os, sys

path, root = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()
print(json.dumps([{"path": os.path.relpath(path, root), "text": text}]))
```

`ddl` is a whole SQL batch, so the row table, the vector index and the two
triggers that keep them in sync all arrive together in `.dirsql.toml`:

```toml
[[table]]
name    = "notes"
glob    = "notes/*.md"
on-file = "python3 note.py {path} {root}"
ddl     = '''
CREATE TABLE notes (path TEXT, text TEXT);

CREATE VIRTUAL TABLE notes_vec
  USING vec0(embedding float[512] distance_metric=cosine);

CREATE TRIGGER notes_vec_ai AFTER INSERT ON notes
WHEN new.text IS NOT NULL BEGIN
  INSERT INTO notes_vec(rowid, embedding)
  VALUES (new.rowid, embed(new.text, 'minishlab/potion-retrieval-32M'));
END;
CREATE TRIGGER notes_vec_ad AFTER DELETE ON notes BEGIN
  DELETE FROM notes_vec WHERE rowid = old.rowid;
END;
'''
```

Three details earn their keep:

- **`distance_metric=cosine`.** `vec0` defaults to L2, which ranks these
  vectors differently. With cosine declared, `notes_vec.distance` is exactly
  what `vec_distance_cosine()` computes in the subquery shape.
- **`WHEN new.text IS NOT NULL`.** `embed(NULL)` is `NULL`, and `vec0` rejects
  a NULL vector outright — without the guard, one unreadable file fails the
  whole table load (`Inserted vector for the "embedding" column is invalid`).
  The guard is this shape's counterpart to the subquery's
  `WHERE emb IS NOT NULL`.
- **No `[[dirsql.extension]]` entry.** The plugin ships its own config
  fragment declaring both `sqlite-vec` and `embed()`, and the launcher
  [loads it automatically](../reference/cli.md#plugins). Only the `[[table]]`
  is yours to write.

### 3. Ask a question

Match against the index, join back to the row table on `rowid`, and let
`sqlite-vec` do the top-k:

```bash
uvx --with dirsql-plugin-embeddings dirsql query "
  SELECT notes.path, notes_vec.distance
  FROM notes_vec
  JOIN notes ON notes.rowid = notes_vec.rowid
  WHERE notes_vec.embedding MATCH embed('how do I cook pasta?',
                                        'minishlab/potion-retrieval-32M')
    AND k = 3
  ORDER BY distance" -c ./.dirsql.toml --persist
```

```json
[{"path":"notes/pasta.md","distance":0.315},{"path":"notes/tomatoes.md","distance":0.881},{"path":"notes/branches.md","distance":0.92}]
```

`MATCH … AND k = 3` is `sqlite-vec`'s KNN form: `k` is the number of nearest
neighbours to return, and the virtual table supplies a `distance` column for
each. Embed the question with the **same model id the trigger used** — a
vector from another model is a different space, and the ranking it produces is
noise rather than an error.

### What the triggers guarantee

dirsql writes file rows with plain `INSERT` and `DELETE` — an update is a
delete and an insert in one transaction, and there is no `UPDATE` path on user
rows — so those two triggers cover every case. The initial load fills the
index; each [watcher](./react-to-changes.md) event maintains it; deleting a
file removes its row *and* its vector. You maintain nothing, and nothing goes
stale.

### Changing the model

The model id is a literal inside the `ddl` string, and the config hash covers
the whole batch. Editing the id therefore invalidates a
[persistent cache](./persist.md) and forces a full re-scan and re-embed
([batch `ddl`](../reference/config.md#batch-ddl)). That is the whole story for
model changes: no stale vectors, no ownership tracking, no migration step.

## Without config: rank a glob inline

For a one-off question, skip all of the above. The subquery scans a
[path-table](../reference/path-tables.md) and embeds each matched file's
`content` on the spot:

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

1. The subquery scans the path-table `'./notes/*.md'` and embeds each file's
   [`content`](../reference/path-tables.md#columns) — only the files the glob
   matches are ever read or embedded. In hand-written SQL the `./` prefix is
   [required](../reference/path-tables.md#writing-the-path); only the
   one-liner normalizes a bare glob for you.
2. `embed('how do I cook pasta?')` embeds the question once (the function is
   deterministic, so SQLite reuses the value across rows).
3. `WHERE emb IS NOT NULL` drops the files that could not be embedded. A
   file that is unreadable or not valid UTF-8 has
   [`NULL` content](../reference/path-tables.md#columns), so its embedding
   and its distance are NULL too — and SQLite sorts NULLs *first* ascending,
   so without this line the unrankable files take the top-k slots.
4. `vec_distance_cosine(...)` computes cosine distance between the two
   vectors; `ORDER BY distance LIMIT 3` keeps the three nearest. There is no
   `vec0` table here, so `MATCH … AND k =` does not apply — for a plain
   expression, `ORDER BY … LIMIT k` *is* `sqlite-vec`'s documented top-k.

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

The same projection works as an `on-file` hook feeding the indexed shape:
parse the field in the hook, store it as the table's `text` column, and the
trigger embeds it once instead of on every query.

## Repeat runs are cheap

Computed vectors are cached on disk, keyed on content and model
([vector cache](../plugins.md#vector-cache)) — re-running a search over
unchanged files skips the model entirely and re-embeds only what changed. That
cuts the *inference* out of the subquery shape, but not the walk or the
per-file round trip; only the `vec0` index removes those. And the plugin costs
nothing when idle: a query that never calls `embed()` spawns no worker and
loads no model ([zero cost when unused](../plugins.md#zero-cost-when-unused)).

## How `embed()` gets into SQL

The plugin ships a config fragment declaring `embed()` via
[`[[dirsql.function]]`](../reference/config.md#dirsql-function), which the
`uvx`/`pip` launcher [discovers automatically](../reference/cli.md#plugins).
The same mechanism is open to your own configs and plugins — any external
command that speaks the
[worker protocol](../reference/config.md#worker-protocol) can back a SQL
function. To build one, see [Write a plugin](./write-a-plugin.md).
