# Add a search index to a table

Make a declared table answer search queries instead of scanning: a B-tree for
lookups, an FTS5 index for keywords, a `vec0` index for meaning. All three are
statements in the table's [`ddl` batch](../reference/config.md#batch-ddl), so
SQLite does the work and `dirsql` maintains nothing.

The templates below are meant to be pasted and edited. Both are shaped around
one worked example — notes in `notes/*.md`, parsed into a `notes` table by an
`extract.py` that prints `{"slug": …, "title": …, "body": …}` per file, exactly
as in [Define tables for your files](./define-tables.md).

## Why triggers, and why only two

`dirsql` writes file rows with plain `INSERT` and `DELETE`; an update is a
delete and an insert in one transaction, and there is no `UPDATE` path on user
rows. So an insert trigger and a delete trigger cover every way a row can
change — during the initial build, on each [watcher](./react-to-changes.md)
event, and when a [persistent cache](./persist.md) reconciles a tree that
moved on. There is no third trigger to write and no maintenance command to run.

The triggers are also the part that goes wrong, which is why the templates
spell them out rather than hiding them behind an abstraction.

## Keyword search (FTS5)

FTS5 ships inside SQLite — no extension, no install. Declare an
external-content index beside the row table and let the two triggers feed it:

```toml
[[table]]
name    = "notes"
glob    = "notes/**/*.md"
on-file = "python3 extract.py {path}"
ddl     = '''
CREATE TABLE notes (slug TEXT, title TEXT, body TEXT);

CREATE VIRTUAL TABLE notes_fts USING fts5(
  body,
  content='notes',
  content_rowid='rowid',
  tokenize='porter unicode61'
);
CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
  INSERT INTO notes_fts(rowid, body) VALUES (new.rowid, new.body);
END;
CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
  INSERT INTO notes_fts(notes_fts, rowid, body)
    VALUES ('delete', old.rowid, old.body);
END;
'''
```

`content='notes'` makes the index store only its own search structures and
read column values back from `notes`, so the text is not duplicated.
`tokenize='porter unicode61'` stems English on top of the default
case-folding, so *deploying* matches *deploy*; drop the `porter` half to match
whole words only.

Query the index and join back to the row table on `rowid`:

```bash
dirsql query "
SELECT n.slug,
       bm25(notes_fts) AS score,
       snippet(notes_fts, 0, '[', ']', '…', 8) AS excerpt
FROM notes_fts JOIN notes AS n ON n.rowid = notes_fts.rowid
WHERE notes_fts MATCH 'deploy'
ORDER BY score" -c ./.dirsql.toml
```

```json
[{"excerpt":"Rebase before you [deploy]. Keep pull requests small.","score":-1.0476190476190478e-6,"slug":"branches"},
 {"excerpt":"…the spaghetti goes in. [Deploy] the garlic late.","score":-8.461538461538463e-7,"slug":"pasta"}]
```

**`bm25()` returns negative scores, and better matches are more negative** —
so `ORDER BY score` ascending is best-first, with no `DESC`. `snippet()`'s
arguments are the table, the column index, the open and close markers, the
ellipsis, and the token budget.

### Check the delete trigger

A wrong or missing delete trigger fails **silently**: the index keeps rows for
files that are gone, and they keep matching. Nothing errors.

The symptom is a hit that has no row behind it, which a `LEFT JOIN` exposes:

```bash
dirsql query "
SELECT n.slug
FROM notes_fts LEFT JOIN notes AS n ON n.rowid = notes_fts.rowid
WHERE notes_fts MATCH 'frost'" -c ./.dirsql.toml --persist
```

```json
[{"slug":null}]
```

A `null` slug is a stale index entry — the base row is gone and the index did
not hear about it. With the `notes_ad` trigger above, the deleted file's entry
is gone too and the query returns nothing.

Note the `--persist` flag: without it every run rebuilds the index from
scratch, so a broken delete trigger cannot show itself. Deletes are visible
against a [warm cache](./persist.md) and through the watcher, which is exactly
where the staleness would have bitten.

## Vector search (`vec0`)

Two pieces beyond SQLite: the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension supplies the
`vec0` virtual table, and
[`dirsql-plugin-embeddings`](../plugins.md#dirsql-plugin-embeddings) supplies
`embed()`. Installing the plugin brings both — its config fragment declares
the extension too, and the launcher
[discovers it](../reference/cli.md#plugins):

```bash
uvx --with dirsql-plugin-embeddings dirsql query "…" -c ./.dirsql.toml
```

Loading `sqlite-vec` by hand instead (another runtime, a pinned build) is
[Load a SQLite extension](./load-extension.md).

### Find your model's dimension

A `vec0` column declares a fixed width, so the template needs the number of
components your model emits. `embed()` returns the vector as JSON text, so ask
it:

```bash
uvx --with dirsql-plugin-embeddings dirsql query \
  "SELECT json_array_length(embed('probe', 'minishlab/potion-retrieval-32M')) AS dims"
```

Run it once per model id you intend to use, and substitute the answer for the
`512` in the template below. (The first call for a model downloads it — see
[model](../plugins.md#model).) Getting it wrong is at least loud — the build fails naming both
numbers:

```
dirsql query: failed to load config: SQLite error: Dimension mismatch for
inserted vector for the "embedding" column. Expected 8 dimensions but received 4.
```

### The template

```toml
[[table]]
name    = "notes"
glob    = "notes/**/*.md"
on-file = "python3 extract.py {path}"
ddl     = '''
CREATE TABLE notes (slug TEXT, title TEXT, body TEXT);

-- Width must equal what the probe printed for the model id named below.
CREATE VIRTUAL TABLE notes_vec
  USING vec0(embedding float[512] distance_metric=cosine);
CREATE TRIGGER notes_vi AFTER INSERT ON notes
WHEN new.body IS NOT NULL BEGIN
  INSERT INTO notes_vec(rowid, embedding)
    VALUES (new.rowid, embed(new.body, 'minishlab/potion-retrieval-32M'));
END;
CREATE TRIGGER notes_vd AFTER DELETE ON notes BEGIN
  DELETE FROM notes_vec WHERE rowid = old.rowid;
END;
'''
```

The delete side is an ordinary `DELETE`, not FTS5's `'delete'` command row —
`vec0` is a normal-looking table that way.

Two clauses in there are load-bearing:

- **`distance_metric=cosine`.** `vec0` defaults to L2, which is not the metric
  the rest of dirsql's semantic search uses — `vec_distance_cosine()` backs
  both the plugin's one-liner and
  [Search documents by meaning](./search-by-meaning.md). The two agree only for
  vectors of equal length, so as soon as documents embed to vectors of
  different magnitude they rank differently: against three notes, L2 returned
  `a, c, b` where cosine returned `a, b, c`. Declaring the metric makes
  `distance` the number `vec_distance_cosine()` would compute.
- **`WHEN new.body IS NOT NULL`.** `embed(NULL)` is `NULL`, and `vec0` rejects
  a NULL vector outright — so without the guard a single row whose text is
  missing fails the **whole** table load, not just its own insert:

  ```
  dirsql query: failed to load config: SQLite error: Inserted vector for the
  "embedding" column is invalid: Input must have type BLOB (compact format) or
  TEXT (JSON), found NULL
  ```

  With it, that row still lands in `notes`; only its vector is skipped. FTS5
  needs no such guard — it indexes a NULL happily.

Query it with `sqlite-vec`'s KNN form, joined back on `rowid`:

```bash
uvx --with dirsql-plugin-embeddings dirsql query "
SELECT n.slug, v.distance
FROM notes_vec AS v JOIN notes AS n ON n.rowid = v.rowid
WHERE v.embedding MATCH embed('how do I cook pasta?', 'minishlab/potion-retrieval-32M')
  AND k = 3
ORDER BY v.distance" -c ./.dirsql.toml
```

`MATCH` plus `k = 3` is what makes this a top-k scan rather than a full one —
`vec0` uses `k`, not `LIMIT`. `distance` is supplied by the virtual table,
closest first.

**Name the same model id in the trigger and the query.** Nothing checks that
the two agree, and vectors from different models are not comparable. If the
models happen to share a width the mismatch is entirely silent — rankings just
get worse. Spelling the id out in both places, rather than leaning on the
default in either, is the cheap defence; it also puts the model in the
[config hash](../reference/config.md#batch-ddl), so changing it rebuilds the
index instead of mixing old vectors with new.

Rows are embedded once, at ingest, and cached on disk by content and model
([vector cache](../plugins.md#vector-cache)). A search then costs one
`embed()` call for the query text plus an in-process scan.

::: warning Editing `ddl` wedges a persisted `vec0` cache
Under `--persist`, editing any part of a `ddl` batch whose cache holds a
`vec0` table currently fails with `SQLite error: no such module: vec0`, and
keeps failing until the cache is deleted (`rm -rf <root>/.dirsql`). Tracked in
[#1008](https://github.com/thekevinscott/dirsql/issues/1008); FTS5 is
unaffected.
:::

## What a rebuild does

`ddl` runs once, when the table is created. Editing any character of it
changes the config hash, which drops a persisted cache and re-ingests every
file — so a new index, a different tokenizer or a changed model id all rebuild
from scratch rather than leaving a half-migrated index behind. The full rules
are in [Batch `ddl`](../reference/config.md#batch-ddl).

## Going further

- Ranked semantic search over files with no config at all —
  [Search documents by meaning](./search-by-meaning.md).
- Why indexes belong to declared tables and not to path-tables —
  [how `dirsql` thinks](../explanation.md).
