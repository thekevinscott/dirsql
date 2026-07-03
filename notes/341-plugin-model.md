# A lightweight plugin model (#341)

Design proposal for [#341](https://github.com/thekevinscott/dirsql/issues/341).
Goal: make a hook-backed capability one-click installable, adding as little
machinery as possible on top of Epic B (#322).

## Summary

A plugin is **an ordinary ecosystem package** (pip / npm) that ships hook
scripts as console commands. There is no `dirsql plugin` subcommand, no
fetching, no vendoring, no config rewriting — distribution and dependency
resolution are entirely the package manager's job:

```bash
uvx --with dirsql-plugin-embeddings dirsql
# or
npx -y --package dirsql-plugin-embeddings dirsql
```

`--with` puts the plugin's console scripts on the spawned environment's PATH,
and hook commands inherit `dirsql`'s environment (already part of the
command-execution contract) — so the hooks in `.dirsql.toml` can invoke the
plugin's commands by name. At runtime there is **no plugin system at all**:
the core stays a command runner.

## Anatomy of a plugin

A pip package `dirsql-plugin-embeddings` exposing one console script with a
subcommand per hook:

```
dirsql_plugin_embeddings/
  pyproject.toml     # deps: sentence-transformers, …  entry point: dirsql-embeddings
  main.py            # on-file / pre-query subcommands
  README.md          # the .dirsql.toml snippet to paste
```

Wiring it is the snippet from the plugin's README:

```toml
[[table]]
ddl     = "CREATE TABLE embeddings (_path TEXT, chunk TEXT, embedding TEXT)"
glob    = "**/*.md"
on-file = "dirsql-embeddings on-file {path}"

[dirsql]
pre-query = "dirsql-embeddings pre-query {args}"
```

Key choices:

- **The package manager is the installer.** Deps (models, tokenizers, numpy)
  live in the plugin package's own metadata; `uvx --with` / `npx --package`
  resolve them. `dirsql` never fetches, vendors, or installs anything.
- **The config is literal hook config.** What a plugin can do is by definition
  what a hand-written hook can do — the docs for
  `on-file`/`pre-query`/`post-query` *are* the plugin API. No manifest, no new
  placeholder, no new vocabulary.
- **Naming convention, not registry:** `dirsql-plugin-<name>` on PyPI/npm,
  exposing a `dirsql-<name>` command. Discoverable by search; nothing to
  maintain.
- **Alternative invocation without `--with`:** a hook command can carry its
  own deps per-invocation (`on-file = "uvx --from dirsql-plugin-embeddings
  dirsql-embeddings on-file {path}"`). Slower per spawn (resolution is
  cached but checked), so the launch-time `--with` is the documented default.

## Trust

Consent is structural rather than interactive: nothing activates by mere
installation. Running with `--with` *and* pasting the snippet are both
explicit user actions, and the snippet shows the exact command lines that
will run — the same threat model as hand-writing a hook, because it *is*
hand-writing a hook. No sandboxing, same as every other hook.

## The motivating case rides on existing features

The embeddings plugin needs **no new code anywhere** — every line of its
snippet is documented config today:

- `dirsql-embeddings on-file {path}`: reads the file, chunks it, prints
  `[{"chunk": …, "embedding": "[0.12, …]"}, …]`; stat virtuals merge `_path` in.
- `dirsql-embeddings pre-query {args}`: treats the request body as a
  natural-language query, computes its embedding in-process, and prints
  `SELECT _path, chunk, vec_distance_cosine(embedding, '<query vec>') AS score
  FROM embeddings ORDER BY score LIMIT 10`.
- The snippet can include a `[[dirsql.extension]]` entry to load sqlite-vec,
  using the existing package-name resolution in the pip/npm CLIs. A
  pure-Python fallback (cosine in the hook) also works; the extension path is
  just faster.

## Possible later sugar (not v0)

If pasting the snippet proves to be real friction, one small addition closes
it without an installer:

```toml
[dirsql]
plugins = ["dirsql-plugin-embeddings"]
```

Each named package ships a config fragment; the pip/npm launcher resolves the
package (the same binding-layer seam that already resolves extension package
names — the standalone Rust binary stays file-path-only, same caveat) and the
fragment is merged in-memory at config load. Uninstall = delete the line.
This keeps `.dirsql.toml` untouched by tooling but does add a small amount of
config-load surface, so it stays out of v0 until the copy-paste friction is
demonstrated.

## Open questions → positions

- **Distribution:** PyPI/npm under the `dirsql-plugin-*` convention. No
  registry, no git fetching, no local-path machinery — a local plugin is just
  a path-installed package (`uv pip install -e …`).
- **Manifest:** none. The package's own metadata carries deps; the README
  carries the snippet.
- **Config merge/uninstall:** nothing merges; the user owns `.dirsql.toml`.
  Removal = delete the snippet and drop the `--with`.
- **Core support needed:** none for v0. The `plugins` key sugar, if it ever
  lands, touches the binding-layer config load only.
- **Hook chaining:** `pre-query`/`post-query` are server-wide and
  single-valued, so two plugins cannot hook the same query event. Punted:
  that is a hook-substrate question (#322 follow-up), not a plugin one.

## Non-goals (unchanged from the issue)

A registry, a stable plugin API/manifest spec, cross-SDK parity, SDK runtime
changes.
