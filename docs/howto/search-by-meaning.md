# Search documents by meaning

Ask a question in plain language and get the closest documents back — even
when they share no keywords with it. Three pieces you already have compose
into semantic search: a SQLite vector extension for the distance math, an
[`on-file`](../reference/hooks.md#on-file) command to embed each file at
index time, and a [`pre-query`](../reference/hooks.md#pre-query) command to
embed each question at query time.

::: tip Just want it working?
[`dirsql-plugin-embeddings`](https://pypi.org/project/dirsql-plugin-embeddings/)
packages exactly what this guide builds, ready to install:
`uvx --with dirsql-plugin-embeddings dirsql server`. Keep reading to see how it's
built — the same three pieces, from scratch.
:::

## How the pieces fit

1. **[`[[dirsql.extension]]`](../reference/config.md#dirsql-extension)**
   loads [`sqlite-vec`](https://github.com/asg017/sqlite-vec), making
   `vec_distance_cosine()` callable in queries.
2. **`on-file`** runs an embedding command once per matched file; the
   vector lands in an `embedding` column next to the text.
3. **`pre-query`** receives each raw `POST /query` body, embeds the
   question, and prints the nearest-neighbor SQL to run.

`dirsql` never sees a model — both hooks are commands you own, so any
embedding model works. This guide uses
[`model2vec`](https://github.com/MinishLab/model2vec), a small, fast,
CPU-only embedding library (its `potion-base-8M` model downloads ~30 MB
from Hugging Face on first use).

## 1. The embedding scripts

Suppose short notes live in `notes/*.md`:

```
notes/pasta.md      # boiling spaghetti, olive oil, garlic
notes/branches.md   # git feature branches and pull requests
notes/tomatoes.md   # planting tomato seedlings after the last frost
```

Next to them, `embed.py` turns one file into one row carrying its text and
its `path`, text, and embedding (a JSON array, stored as TEXT — `sqlite-vec`
accepts JSON vectors directly). dirsql injects no columns, so the script emits
the path itself, deriving it from the `{path}`/`{root}` the hook passes in:

```python
"""Embed one file's text; print a dirsql row array on stdout."""
import json
import os
import sys

from model2vec import StaticModel

path, root = sys.argv[1], sys.argv[2]
text = open(path, encoding="utf-8").read()
model = StaticModel.from_pretrained("minishlab/potion-base-8M")
vector = model.encode([text])[0]
row = {"path": os.path.relpath(path, root), "text": text,
       "embedding": json.dumps([round(float(x), 6) for x in vector])}
print(json.dumps([row]))
```

And `search.py` turns a `{"q": "..."}` request body into SQL, embedding the
question with the same model:

```python
"""Turn a {"q": "..."} request body into a nearest-neighbor SQL query."""
import json
import sys

from model2vec import StaticModel

body = json.loads(sys.argv[1])
model = StaticModel.from_pretrained("minishlab/potion-base-8M")
vector = model.encode([body["q"]])[0]
needle = json.dumps([round(float(x), 6) for x in vector])
print(
    "SELECT path, ROUND(vec_distance_cosine(embedding, '%s'), 3) AS distance "
    "FROM notes ORDER BY distance LIMIT 3" % needle
)
```

::: warning The hook owns SQL safety
Whatever SQL `pre-query` prints is executed as-is. Here the interpolated
value is a numeric vector the script itself produced; never splice raw
request text into SQL. See [`pre-query`](../reference/hooks.md#pre-query).
:::

## 2. Wire them up in `.dirsql.toml`

```toml
[dirsql]
pre-query = "uv run --with model2vec python search.py {args}"
hook-timeout = 300   # headroom for the first-run model download

[[dirsql.extension]]
path       = "sqlite_vec"          # Python module name; see note below
entrypoint = "sqlite3_vec_init"

[[table]]
ddl     = "CREATE TABLE notes (path TEXT, text TEXT, embedding TEXT)"
glob    = "notes/*.md"
on-file = "uv run --with model2vec python embed.py {path} {root}"
```

The extension is named by package: the Python launcher resolves the
installed `sqlite_vec` module to its bundled loadable. Naming rules per
runtime — and the literal-path alternative that works everywhere — are in
[Load a SQLite extension](./load-extension.md).

## 3. Ask questions

Run with `sqlite-vec` available to the launcher's environment. The initial
scan runs `embed.py` once per note, then the query argument goes straight to
`pre-query`, exactly as a `POST /query` body would. Pass the config with
[`-c`](../reference/cli.md#flags) — `dirsql` does not auto-load a
`.dirsql.toml` from the current directory:

```bash
uvx --with sqlite-vec dirsql query '{"q": "how do I cook pasta?"}' -c ./.dirsql.toml
```

```json
[{"path":"notes/pasta.md","distance":0.315},{"path":"notes/tomatoes.md","distance":0.881},{"path":"notes/branches.md","distance":0.92}]
```

```bash
uvx --with sqlite-vec dirsql query '{"q": "reviewing code on github"}' -c ./.dirsql.toml
```

```json
[{"path":"notes/branches.md","distance":0.51},{"path":"notes/pasta.md","distance":1.033},{"path":"notes/tomatoes.md","distance":1.074}]
```

Neither question shares a keyword with its top note — "cook" appears
nowhere in `pasta.md`, "github" nowhere in `branches.md`. The distance
ranking is doing the work.

Because `pre-query` is set, the query argument is *not* the usual
`{"sql": …}` — it goes to your script as-is, which decides what SQL runs
([hook interactions](../reference/http-api.md#hook-interactions)).

## Recomputing vs. caching

Embeddings are recomputed on every startup, because the database is a
derived, ephemeral view of the files
([how `dirsql` thinks](../explanation.md)). Once the model is cached this
is fast for small trees; for large ones, enable persistence so unchanged
files keep their stored embeddings across restarts —
[Keep the index across restarts](./persist.md). Editing a note re-embeds
just that file, automatically.
