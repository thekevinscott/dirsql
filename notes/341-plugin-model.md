# A lightweight plugin model (#341)

Design proposal for [#341](https://github.com/thekevinscott/dirsql/issues/341).
Goal: make a hook-backed capability one-click installable, adding as little
machinery as possible on top of Epic B (#322).

## Summary

A plugin is **a directory: a descriptor plus scripts**. Installing one vendors
the directory into `.dirsql/plugins/<name>/` and merges its config fragment
into `.dirsql.toml`. At runtime there is **no plugin system at all** — the
merged config is ordinary hook config, and the core stays a command runner.
The entire feature is a CLI-side installer: fetch, consent, merge, unmerge.

## Anatomy of a plugin

```
embeddings/
  plugin.toml       # descriptor
  embed_file.py     # on-file hook
  to_sql.py         # pre-query hook
```

`plugin.toml`:

```toml
[plugin]
name        = "embeddings"
description = "Semantic search over indexed files"
requires    = ["uv"]   # binaries that must be on PATH; preflight-checked at install

# The exact TOML to merge into .dirsql.toml — no plugin vocabulary to learn.
[[config.table]]
ddl     = "CREATE TABLE embeddings (_path TEXT, chunk TEXT, embedding TEXT)"
glob    = "**/*.md"
on-file = "uv run --with sentence-transformers {plugin}/embed_file.py {path}"

[config.dirsql]
pre-query = "uv run --with sentence-transformers {plugin}/to_sql.py {args}"
```

Key choices:

- **The fragment is literal config.** `[config]` holds exactly the TOML that
  lands in `.dirsql.toml`, so what a plugin can do is by definition what a
  hand-written hook can do — the docs for `on-file`/`pre-query`/`post-query`
  *are* the plugin API.
- **One placeholder, resolved at install time.** `{plugin}` expands to the
  vendored directory path when the fragment is merged — plain string
  substitution in the CLI. The runtime placeholder set (`{path}`, `{args}`, …)
  is untouched, so there is zero new runtime surface.
- **Dependencies stay out-of-process and per-ecosystem.** The command string
  itself carries them (`uv run --with …`, `npx -y …`). `dirsql` never installs
  packages; `requires` is just a `which` check with a friendly error.

## CLI surface

```
dirsql plugin add <local-path | git-url>   # vendor + consent + merge
dirsql plugin remove <name>                # unmerge + delete vendored dir
dirsql plugin list
```

`add` does three things:

1. **Fetch.** A local path is copied, a git URL shallow-cloned, into
   `.dirsql/plugins/<name>/`. `.dirsql/` is already the reserved, never-scanned
   namespace, so vendored scripts can never leak into a table.
2. **Consent.** Print the full config fragment — i.e. the exact command lines
   that will execute — and require an interactive `y` (or `--yes` for
   automation). This is the trust story: same threat model as hand-writing a
   hook, but the commands are shown, not buried. Refuse if a server-wide key
   (`pre-query` / `post-query`) is already set by the user or another plugin —
   those keys are single-valued; chaining is out of scope.
3. **Merge.** Edit `.dirsql.toml` with `toml_edit` (format- and
   comment-preserving), tagging each inserted item
   (`# managed by dirsql plugin: embeddings`) and recording the inserted
   keys/tables in the vendored copy of `plugin.toml`.

`remove` deletes exactly the recorded items, then the vendored directory. Hand
edits elsewhere survive because edits are surgical, never a re-serialization.
`add` on an installed plugin errors; `--force` = remove + add.

## The motivating case rides on existing features

The embeddings plugin needs **no new core code** — every line of its fragment
is documented config today:

- `embed_file.py {path}` (via `on-file`): reads the file, chunks it, prints
  `[{"chunk": …, "embedding": "[0.12, …]"}, …]`; stat virtuals merge `_path` in.
- `to_sql.py {args}` (via `pre-query`): treats the request body as a natural-
  language query, computes its embedding in-process, and prints
  `SELECT _path, chunk, vec_distance_cosine(embedding, '<query vec>') AS score
  FROM embeddings ORDER BY score LIMIT 10`.
- The fragment can also carry a `[[config.dirsql.extension]]` entry to load
  sqlite-vec, using the existing package-name resolution in the pip/npm CLIs.

A pure-Python fallback (cosine in the hook, no extension) also works; the
extension path is just faster.

## Open questions → positions

- **Distribution:** v0 is local path + git URL. No registry. A
  `gh:user/repo` shorthand is a cheap later add.
- **Manifest:** a separate `plugin.toml` inside the plugin dir — never extra
  keys in `.dirsql.toml`, which stays purely config the user owns.
- **Trust:** install-time consent showing the exact commands, plus the
  single-valued-key refusal. No sandboxing — hand-written hooks have none
  either, and pretending otherwise would be false comfort.
- **Merge/uninstall:** `toml_edit` + a per-plugin record of inserted items
  (above). Idempotent by construction.
- **Core support needed:** none. Everything lives under the `cli` Cargo
  feature (`packages/rust/src/cli/`), which is never compiled into the SDK
  bindings — no SDK surface, no PARITY drift, and per the `cli`-only carve-out
  (#337/#328), no binding re-attestation.

## Known limitation

`pre-query`/`post-query` are server-wide and single-valued, so two plugins
cannot both hook the same query event. Punted: chaining/multiplexing is a
hook-substrate question (#322 follow-up), not a plugin-installer one.

## Non-goals (unchanged from the issue)

A registry, a stable plugin API/manifest spec, cross-SDK parity, SDK runtime
changes.
