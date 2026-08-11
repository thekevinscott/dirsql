# Write a plugin

Ship a ready-made set of `dirsql` tables — an embeddings index, a
log-line extractor, a metrics view — so a teammate gets them by installing a
package, with zero config edits. A **plugin** is an ordinary pip package that
carries a `dirsql.toml` config fragment and declares itself with a `dirsql`
entry point; when it is installed in the same environment as the `uvx`/`pip`
launcher, the launcher [discovers it and loads its fragment
automatically](../reference/cli.md#plugins). Nothing in `dirsql` is
plugin-aware — a plugin is just config plus a naming convention.

## Package layout

A plugin is a normal Python package. Two things make it a plugin:

1. A **`dirsql.toml`** fragment shipped inside a top-level module.
2. A **`[project.entry-points.dirsql]`** declaration pointing the launcher at
   that module.

```
dirsql-embeddings/
├── pyproject.toml
└── src/
    └── dirsql_embeddings/
        ├── __init__.py
        ├── dirsql.toml       # the config fragment
        └── embed.py          # on-file hook
```

The entry point maps a **source label** (the name) to the **module that
ships `dirsql.toml`** (the value):

```toml
# pyproject.toml
[project]
name = "dirsql-embeddings"
version = "0.1.0"
dependencies = ["model2vec", "sqlite-vec"]

[project.entry-points.dirsql]
embeddings = "dirsql_embeddings"
```

The entry-point **name** (`embeddings`) is the source label the launcher uses
to identify the plugin in diagnostics; the **value** (`dirsql_embeddings`) is
the importable module the launcher resolves to find `dirsql.toml` beside it.
Installing the package is all it takes — there is no enable step and no
filename convention beyond `dirsql.toml`.

## The fragment is an ordinary config

A plugin's `dirsql.toml` is a plain [config file](../reference/config.md) —
structurally **identical to a user config**. It may declare
[`[[table]]`](../reference/config.md#table) (with
[`on-file`](../reference/hooks.md#on-file)),
[`[[dirsql.extension]]`](../reference/config.md#dirsql-extension),
[`ignore`](../reference/config.md#dirsql-keys), and
[`hook-timeout`](../reference/hooks.md#timeout).

There are **no plugin-specific keys and no plugin-specific restrictions**. The
config schema is content-only: the index `root` and `--persist` are
[runner-owned flags](../reference/config.md#dirsql-keys) (`--root`, `--persist
[PATH]`), decided by whoever runs `dirsql`, never by a config file — so a
plugin has nothing to say about them. Whatever you can put in your own
`.dirsql.toml`, a plugin can put in its fragment, and vice-versa.

## Hook commands

Both hook command styles from the [hook contract](../reference/hooks.md) work
in a plugin fragment:

- **Console scripts** — a `bin`-style entry point your package installs on
  `PATH` (`embed-file {path} {root}`). Recommended for published plugins: the command
  is bound to your package's interpreter and dependencies, and it is
  language-neutral (the fragment names a command, not a Python file).
- **Relative scripts** — a path resolved against the fragment's own directory
  (`uv run python embed.py {path}`). Convenient while developing the plugin
  in-tree.

Two facts from the [execution contract](../reference/hooks.md#execution-contract)
matter most for a published plugin:

- **A hook runs in its declaring config's directory.** For a plugin that is
  the installed fragment's directory — inside **site-packages**. That is a
  read-only, shared location: **run from it, never write to it.** Use the
  absolute [`{path}`](../reference/hooks.md#on-file) placeholder to read the
  matched file, and [`{root}`](../reference/hooks.md#on-file) to reach the
  user's project directory. Write any cache to `{root}` or a real cache dir,
  never next to the fragment.
- **`{path}` is absolute and `{root}` is the index root**, so a command is
  self-sufficient from any working directory — it works whether the plugin
  lives in the project or in site-packages.

## Worked example: an embeddings plugin

Here is the whole plugin — vector search over a directory of notes, buildable
in about forty lines. It composes two pieces: the
[`sqlite-vec`](https://github.com/asg017/sqlite-vec) extension for the
distance math, and an `on-file` command to embed each file.

The fragment, `src/dirsql_embeddings/dirsql.toml`:

```toml
[dirsql]
hook-timeout = 300   # headroom for the first-run model download

[[dirsql.extension]]
path       = "sqlite_vec"
entrypoint = "sqlite3_vec_init"

[[table]]
ddl     = "CREATE TABLE notes (path TEXT, text TEXT, embedding TEXT)"
glob    = "notes/*.md"
on-file = "uv run --with model2vec python embed.py {path} {root}"
```

`embed.py` turns one file into one row carrying its path, text, and embedding.
dirsql injects no columns, so the script emits the path itself, from the
`{path}`/`{root}` the hook passes in:

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

The relative `embed.py` above resolves against the fragment directory, which
is convenient during development. For a published plugin, promote it to a
console script (`[project.scripts]` → `embed-file`) so the command carries
its own interpreter and dependencies and no longer depends on
`uv run --with`.

Once the package is installed alongside the launcher, its `notes` table is
queryable with no config edits:

```bash
uvx --with dirsql-embeddings dirsql query \
  "SELECT path FROM notes ORDER BY vec_distance_cosine(embedding, '[0.1, ...]') LIMIT 3"
```

The launcher discovers the installed plugin, composes its fragment, and the
`notes` table (with `vec_distance_cosine()` from the loaded extension) is
available to the query.

## SDK-style convention: expose the config

The launcher's auto-discovery is [launcher-only](#boundaries): an
[SDK](../reference/sdk.md) consumer never gets a plugin's tables automatically.
The convention that bridges this — **zero `dirsql` code, purely a plugin
courtesy** — is to also expose the fragment's path programmatically, so an
application can pass it to the constructor's
[`config`](../reference/sdk.md#constructor) parameter by hand:

```python
# src/dirsql_embeddings/__init__.py
from importlib.resources import files

def config_path() -> str:
    """Absolute path to this plugin's dirsql.toml, for SDK consumers."""
    return str(files(__package__) / "dirsql.toml")
```

```python
from dirsql import DirSQL
from dirsql_embeddings import config_path

db = DirSQL("./project", config=config_path())
```

This is not a `dirsql` feature — it is a naming convention a well-behaved
plugin follows so its config is reachable both ways: auto-discovered by the
launcher, and hand-passed to an SDK.

## Boundaries

Discovery is deliberately narrow. Know exactly who does what:

- **The `cargo`-installed binary does no discovery.** It loads configs only
  from [`-c/--config`](../reference/cli.md#flags). A standalone binary user
  passes a plugin's fragment explicitly, like any other config.
- **The `uvx`/`pip` launcher injects `-c <fragment>` per installed plugin.**
  Each discovered plugin's fragment is appended after your own `-c` configs, so
  your config still takes ordering precedence
  ([composing configs](../reference/config.md#composing-multiple-configs)). When
  you pass no `-c` of your own, the launcher also keeps the
  shipped starter `records` table (an internal `--include-default`), so plugins
  **add** tables rather than standing alone. Discovery is **pip/uvx only** for now — the `npx`
  launcher does not yet discover — and is switched off per invocation with
  [`--no-plugin` or `DIRSQL_NO_PLUGIN=1`](../reference/cli.md#plugins).
- **The SDK never auto-discovers.** Pass a plugin's config explicitly (the
  [convention above](#sdk-style-convention-expose-the-config)).
- **Name collisions are a hard error.** Because fragments compose like any
  [multiple configs](../reference/config.md#composing-multiple-configs), two
  plugins (or a plugin and your config) defining a table of the same name
  fail loudly, naming the conflict. It is never a silent
  last-writer-wins.

::: warning Config flags are subcommand-local
For `dirsql query`, pass `-c` and friends **after** the subcommand
(`dirsql query "<sql>" -c <cfg>`); before it they are a hard error. Plugin
discovery is unaffected — the launcher injects its `-c` flags in the right
position — but it matters when you also pass a config of your own. See
[the CLI reference](../reference/cli.md#dirsql-query).
:::
