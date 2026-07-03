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

## Trust

Consent is structural rather than interactive: nothing activates by mere
installation. Running with `--with` *and* pasting the snippet are both
explicit user actions, and the snippet shows the exact command lines that
will run — the same threat model as hand-writing a hook, because it *is*
hand-writing a hook. No sandboxing, same as every other hook.

## MVP: two small core additions

The plugin *model* needs zero new code, but the motivating embeddings case
exposed two gaps in the hook substrate that the MVP should close. Both are
config-loader surface parsed by the shared Rust core, so every install gets
them identically (no per-SDK code, no parity drift).

### 1. Configurable hook timeouts

Today every hook run is bounded by a hardcoded 30s (`lib.rs`
`ON_FILE_TIMEOUT`, `router.rs` `PRE_QUERY_TIMEOUT` / `POST_QUERY_TIMEOUT`).
The bound itself is load-bearing — per-file error isolation only works if a
bad command *terminates*, so a hang becomes an ordinary per-file failure —
but the *value* must be configurable: an embedding hook's first run may
download a model, and slow files / rate-limited APIs legitimately exceed 30s.

```toml
[[table]]
on-file = "dirsql-embeddings on-file {path}"
timeout = 300                # seconds; default 30

[dirsql]
pre-query         = "dirsql-embeddings pre-query {args}"
pre-query-timeout = 60       # exact key spelling TBD
```

### 2. Auxiliary schema (`setup-sql`) — the virtual-table answer

`dirsql` rejects `CREATE VIRTUAL TABLE` as a `[[table]]` DDL for a structural
reason: `create_table` injects hidden `_dirsql_` tracking columns and the
engine owns the table's rows (one per file, diffed on change) — an
extension-backed virtual table has its own storage module and can't accept
either. That rejection stands.

But the embeddings case doesn't need a dirsql-*managed* virtual table; it
needs a vec0 **index** kept in sync with a dirsql-managed table. SQLite
already knows how to do that: dirsql maintains its tables via ordinary
`INSERT INTO` / `DELETE FROM` statements (`db.rs`), so **standard SQLite
triggers fire on every row the engine writes**. All that's missing is a way
to declare schema that dirsql executes but does not own:

```toml
[dirsql]
setup-sql = [
  "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(embedding float[384])",
  "CREATE TRIGGER IF NOT EXISTS embeddings_ai AFTER INSERT ON embeddings BEGIN INSERT INTO vec_chunks(rowid, embedding) VALUES (new.rowid, vec_f32(new.embedding)); END",
  "CREATE TRIGGER IF NOT EXISTS embeddings_ad AFTER DELETE ON embeddings BEGIN DELETE FROM vec_chunks WHERE rowid = old.rowid; END",
]
```

Semantics: statements run once per startup, in order, **after** extensions
load and dirsql's tables are created, **before** the scan begins. Statements
must be idempotent (`IF NOT EXISTS`) since they re-run every startup and
after every full cache rebuild. dirsql never reads, diffs, or reconciles
these objects — the triggers keep them in sync as a pure SQLite concern, with
zero changes to the scanner, differ, or persistence reconcile.

This is deliberately generic, not vec-specific: an FTS5 full-text-search
plugin is the identical shape (FTS5 virtual table + sync triggers), as are
materialized summary tables. One small key unlocks the whole
"extension-backed index over dirsql rows" family, and ANN-scale semantic
search stops being a limitation.

Needs a spike to validate: trigger-maintained vec0 sync end-to-end (insert /
delete / re-parse update paths), and behavior under a persistence reconcile.

## The motivating case, end to end

With the two MVP additions, the plugin's snippet is: one `[[table]]` with
`on-file` + `timeout`, the `[[dirsql.extension]]` entry for sqlite-vec
(existing feature, package-name resolution already works via pip/npm),
`setup-sql` for the vec0 index + triggers, and `pre-query` translating a
natural-language body into a `MATCH`-based KNN query. At small scale the
plugin can skip the extension and `setup-sql` entirely and brute-force cosine
in SQL or in the hook.

## Known drawbacks (accepted for MVP)

- **Process-per-file, model-load-per-spawn.** `on-file` spawns per matched
  file; a local model reloads every time, and batching across files is
  impossible. Viable MVP shapes: API-backed embedders (each spawn is a thin
  HTTP call) or a user-managed local inference server. `persist = true` is
  effectively mandatory so re-embedding happens only for changed files.
- **Partial corpora are quiet.** A timed-out or failing file is skipped with
  only a stderr warning — an embedding corpus can be silently incomplete.
- **One plugin owns the query surface.** `pre-query`/`post-query` are
  server-wide and single-valued: the hook must pass raw SQL through itself,
  and two plugins can't both hook queries. Punted as a hook-substrate
  question (#322 follow-up).
- **The query half is HTTP-only.** `pre-query` is wired through the CLI
  `/query` handler; in-process SDK `query()` never sees it. SDK users get the
  embeddings table and indexes (the scan and `setup-sql` run in the shared
  core) but write the vector SQL themselves.
- **stdout discipline.** Hook output framing is stdout-based; a library that
  prints progress to stdout corrupts the parse.
- **Launch coupling.** Running plain `dirsql` without `--with` turns every
  hook into a spawn error (skipped files / 500s) — a confusing failure mode.

## Punted (explicitly out of MVP)

- **Supervised sidecar services** (`[dirsql.service.*]` with start/stop
  lifecycle) — would fix model-load-per-spawn for local models, but every
  sketch so far leans on fragile process conventions (readiness lines on
  stdout, port handoff, signal semantics). Needs a design of its own once the
  MVP demonstrates demand; until then, local-model plugins can point hooks at
  a user-managed inference server.
- **Hook chaining/multiplexing** (multiple plugins on one query event).
- **`plugins = [...]` config sugar** (launcher-resolved config fragments) —
  only if snippet-pasting proves to be real friction.

## Non-goals (unchanged from the issue)

A registry, a stable plugin API/manifest spec, cross-SDK parity, SDK runtime
changes.
