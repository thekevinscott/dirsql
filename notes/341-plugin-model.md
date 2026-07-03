# A lightweight plugin model (#341)

Design proposal for [#341](https://github.com/thekevinscott/dirsql/issues/341).

## Summary

A plugin is **an ordinary ecosystem package** meant to be installable via `uvx` or `npx`:

```bash
uvx --with dirsql-plugin-embeddings dirsql
# or
npx -y --package @dirsql/embeddings dirsql
```

Four requirements define the model:

## 1. Plugins are automatically loaded when present — CLI only

If a plugin package is installed (a Python package / in `node_modules`), the
**CLI** activates it — no snippet-pasting, no registration step. With
`uvx --with dirsql-plugin-embeddings dirsql`, the `--with` *is* the activation
gesture; requiring a config line on top would make it silently do nothing.
Precedent: pytest, flake8, and Datasette (the closest analog) all treat
installed-in-the-environment as active. Escape hatch worth copying from
pytest: a `--no-plugin <name>` flag / env-var disable.

The **SDK does not auto-discover**: a `DirSQL(...)` constructor whose behavior
changes because something appeared in `site-packages` — possibly via a
transitive dependency — is ambient action inside an application (Prettier
shipped exactly this in v2 and removed it in v3). SDK users splice plugin
config explicitly (see requirement 4).

The principled line, which also matches the implementation seam: ambient
discovery is a property of the environment-driven entrypoint (the pip/npm
launcher), never the core or SDKs. Distinct from extension package-name
resolution, which works in SDK config loads too — that resolves something the
config explicitly *names*; discovery activates something named nowhere. The
standalone Rust binary is discovery-free (same caveat as extensions).
Installing a plugin is consenting to run it.

## 2. Plugins define a TOML identical to dirsql's

A plugin ships a `.dirsql.toml` fragment in the same format as the project
config. Merging is **additive**:

- List-shaped config (`[[table]]`, `[[dirsql.extension]]`, `setup-sql`)
  concatenates naturally across plugins and user config.
- Single-valued keys (`pre-query`, `post-query`): **two definitions throw**,
  with an error naming both sources — plugin vs. plugin and plugin vs. user
  config alike. No silent shadowing, no chaining. (Additive hooks as a
  pass-through middleware pipeline is a plausible future direction, but the
  ordering story under auto-loading needs its own design — punted.)
- Plugin TOML is **whitelisted** to tables, extensions, `setup-sql`, and query
  hooks — a plugin may not set `root`, `persist`, `ignore`, or other
  project-owned keys.

```toml
[[table]]
ddl     = "CREATE TABLE embeddings (_path TEXT, chunk TEXT, embedding TEXT)"
glob    = "**/*.md"
on-file = "dirsql-embeddings on-file {path}"

[dirsql]
pre-query = "dirsql-embeddings pre-query {args}"
```

## 3. Plugins ship scripts, resolved appropriately

Hook commands are the plugin's own executables. Packaging solves resolution:
console entry points (`dirsql-embeddings`) need no path resolution at all —
`uvx --with` / `npx --package` put them on the spawned environment's PATH, and
hooks inherit dirsql's environment. Anything else resolves relative to the
package's install location.

## 4. Plugins are language-specific but support both config styles

A plugin targets one ecosystem (PyPI or npm). It should support both
consumption styles where possible:

- **TOML** (the auto-loaded fragment above) — the only style dirsql itself
  implements.
- **SDK** — a zero-dirsql-code authoring convention: the package also exports
  its config programmatically (e.g. `dirsql_plugin_embeddings.tables()`) for
  users to splice into `DirSQL(...)`.

Known gap: `pre-query`/`post-query` exist only on the CLI server, so a
plugin's query-side behavior has no SDK equivalent in either style.

## The motivating case, end to end

Two small core additions make embeddings viable: **configurable hook
timeouts** (#351) and a **`setup-sql`** config key — raw SQL statements dirsql
runs once per startup (after extensions load, before the scan) for schema it
executes but does not own: e.g. a vec0 virtual table plus the sync triggers
that fire on dirsql's own INSERT/DELETE row maintenance.

The plugin's TOML is then: one `[[table]]` with `on-file` + `timeout`, a
`[[dirsql.extension]]` entry for sqlite-vec (existing feature, package-name
resolution already works via pip/npm), `setup-sql` for the vec0 index +
triggers, and `pre-query` translating a natural-language body into a
`MATCH`-based KNN query. At small scale the plugin can skip the extension and
`setup-sql` entirely and brute-force cosine in SQL or in the hook.
