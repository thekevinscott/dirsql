# Migrations

Upgrade guides for `dirsql` consumers. Every release that breaks, removes, or
changes the runtime behavior of a public surface gets its own entry here.

This file is the source of truth. The docs site
([Migrations](https://thekevinscott.github.io/dirsql/migrations)) is generated
from it via a VitePress include; do not edit the rendered page.

See also: [`CHANGELOG.md`](https://github.com/thekevinscott/dirsql/blob/main/CHANGELOG.md) for the full release log. (The relative path is not used because this file is also included into the docs site via a VitePress include, where relative paths would break.)

## [Unreleased]

### CLI launcher directories renamed (`_cli/` -> `cli/`, `src/bin/` -> `src/cli/`)

#### Summary

The Python `dirsql/_cli/` package and the TypeScript `packages/ts/src/bin/`
directory are both renamed to `cli/`. This is a cross-SDK consistency cleanup:
Python's leading underscore was misleading (the directory holds the public
console-script entry point, not internal-only code), and the TypeScript `bin/`
naming did not match Python's `_cli/`. The user-facing `dirsql` command on
PATH is unchanged; only the internal package layout and the published
metadata that points at it move.

Affects:

- **Python:** `[project.scripts] dirsql = "dirsql._cli.main:main"` is now
  `dirsql = "dirsql.cli.main:main"`. Anyone importing `dirsql._cli.*`
  directly (the leading underscore signalled "do not") must update their
  import path.
- **TypeScript:** the npm `bin` field on the `dirsql` package now points at
  `dist/cli/dirsql.js` instead of `dist/bin/dirsql.js`. The `dirsql`
  shim in `node_modules/.bin/` is unchanged.
- **End users running `dirsql ...`:** no change.

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python module imports | `from dirsql._cli.main import main` | `from dirsql.cli.main import main` |
| Python `[project.scripts]` (wheel metadata only; not a consumer-visible API) | `dirsql = "dirsql._cli.main:main"` | `dirsql = "dirsql.cli.main:main"` |
| npm `bin` field (package metadata; not a consumer-visible API) | `"dirsql": "dist/bin/dirsql.js"` | `"dirsql": "dist/cli/dirsql.js"` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

_None._ The `dirsql` command on PATH behaves identically before and after
the rename; only the internal module path used by the wheel's console-script
entry and the npm package's `bin` field have moved.

#### Verification

After upgrading, the `dirsql` command still resolves and runs:

```bash
dirsql --version
# expected: dirsql <version>
```

Python consumers who imported the (underscore-prefixed) launcher modules
directly should update the import path:

```bash
python -c "from dirsql.cli.main import main; print('ok')"
# expected: ok
```

### Python 3.10 support dropped

#### Summary

dirsql's `requires-python` is raised from `>=3.10` to `>=3.11`. `pip` and
`uv` will refuse to install dirsql 0.3.6+ on Python 3.10; 3.10 wheels are
no longer published. This affects only the Python SDK (`dirsql` on PyPI);
the Rust crate and npm package are unchanged.

The driver is release tooling, not a runtime API change. putitoutthere's
multi-version wheel build (#369) fans one wheel row per `requires-python`
version, and its `bundle_cli` wheel-content verify step runs `import
tomllib` — a stdlib module only on CPython >= 3.11 — so the 3.10 row
crashes the release build. Raising `requires-python` removes the 3.10
row. Support can be restored once the upstream verify step no longer
depends on `tomllib`.

#### Required changes

| Before | After |
|--------|-------|
| `pip install dirsql` on Python 3.10 | Upgrade to Python >= 3.11, then `pip install dirsql` |

#### Deprecations removed

_None._

#### Behavior changes without code changes

Installation on Python 3.10 now fails at resolve time (`pip` reports the
package requires a different Python) instead of installing. No change for
Python 3.11+.

#### Verification

On Python 3.11 or newer: `pip install dirsql` resolves and installs as
before. On Python 3.10: `pip install dirsql` exits with
`Requires-Python >=3.11`.

### Content parsing removed; `[table.columns]` / `format` / `each` no longer recognized

#### Summary

dirsql's scope is narrowed to its actual purpose: bridging a local filesystem
to a SQL index. Content interpretation — frontmatter, JSON dot-paths, CSV
parsing, the whole `Format` zoo — is no longer dirsql's job. The `parser.rs`
module and every related symbol are deleted; the `[table.columns]`, `format`,
and `each` keys are no longer part of the `.dirsql.toml` grammar. Affects
every consumer that used `from_config` / `config=` / `new DirSQL(configPath)`
to point at JSON, JSONL, CSV, TSV, TOML, YAML, or markdown-with-frontmatter
files. Closes [#169](https://github.com/thekevinscott/dirsql/issues/169).

Programmatic `Table::new(...)` consumers are unaffected at the call-site
level: their extract callbacks already do their own parsing. They do however
gain auto-injection of glob captures and stat virtuals into each row (see
"Behavior changes without code changes" below).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| `.dirsql.toml` `[[table]]` for parsed content | `ddl = "CREATE TABLE items (name TEXT, price REAL)"` + `glob = "items/*.json"` (relied on JSON parsing) | Move parsing into a programmatic `Table` whose `extract` parses the bytes in your host language. The `.dirsql.toml` entry for filesystem-fact-only tables stays as `ddl = "CREATE TABLE items (_path TEXT, _basename TEXT, ...)"` + `glob`. |
| `.dirsql.toml` `format = "..."` | `format = "json"` (hard requirement when extension didn't match) | Key is no longer recognized. Drop it. To opt into content parsing, write a programmatic `Table` instead. |
| `.dirsql.toml` `each = "..."` | `each = "data.items"` (dot-path navigation into JSON/YAML/TOML) | Key is no longer recognized. Drop it. Use a programmatic `Table` whose extract walks the structure (e.g. `json.loads(content)["data"]["items"]`). |
| `.dirsql.toml` `[table.columns]` | `[table.columns]\ndisplay_name = "metadata.author.name"` | Block is no longer recognized. Drop it. To project nested values into columns, do it in a programmatic `Table` extract. |
| Glob captures in `[[table]]` | Only worked when `[table.columns]` referenced them or relied on implicit dispatch | Captures are auto-injected as columns by name (`thread_id` from `posts/{thread_id}/*.md`) when the DDL declares them. No `[table.columns]` mapping required. |
| `DirSqlError::NoFormat`, `ConfigError::UnknownFormat` (Rust) | Public error variants | Removed. Catch the parent `DirSqlError` / `ConfigError` instead. |

Worked example. Before:

```toml
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, body TEXT)"
glob = "comments/{thread_id}/index.jsonl"
```

After (Python — content parsing moves into the user's code):

```python
from dirsql import DirSQL, Table
import json

db = DirSQL(
    "/path/to/root",
    tables=[
        Table(
            ddl="CREATE TABLE comments (thread_id TEXT, body TEXT)",
            glob="comments/{thread_id}/index.jsonl",
            extract=lambda path: [
                {"body": json.loads(line)["body"]}
                for line in open(path, encoding="utf-8").read().splitlines()
                if line
            ],
        )
    ],
)
```

`thread_id` is auto-injected from the glob capture; the user's extract only
returns `{"body": ...}`.

#### Deprecations removed

_None._ The removed keys (`format`, `each`, `[table.columns]`) and error
variants (`NoFormat`, `UnknownFormat`) were never deprecated; they are
removed in a single release as part of the scope change.

#### Behavior changes without code changes

- **Filesystem-fact auto-injection** is now applied uniformly to every row,
  whether produced by a programmatic or config-defined `Table`. For each
  row the core merges in:
  - glob path captures by capture name (e.g. `thread_id`),
  - stat virtuals under reserved `_`-prefixed names (`_path`, `_basename`,
    `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`).
  Auto-injected keys are filtered to the columns declared in the table's
  DDL, so a strict-mode table with a minimal DDL is not broken by virtuals
  it didn't ask for. User-extract values win over auto-injected values when
  keys collide.

  *Impact on existing programmatic consumers:* if your DDL happens to
  declare a column whose name matches a glob capture or one of the stat
  virtuals (e.g. you had `CREATE TABLE foo (_path TEXT, ...)` in the DDL
  and your extract did **not** populate `_path`), the column is now
  populated automatically. If your extract does populate it, your value
  wins — no change in observable behavior.

- **`.dirsql.toml` files that still contain `format = "..."`, `each =
  "..."`, or `[table.columns]` blocks** parse without error (TOML's default
  permissive deserialization ignores unknown keys). The keys are silently
  dropped. Tables produce filesystem-fact rows regardless. If you relied on
  parsed content, you will see all-NULL or all-default values until you
  migrate to a programmatic `Table` (see "Required changes" above).

#### Verification

```bash
# 1. Confirm the parser module no longer exists in the dependency.
cargo tree -p dirsql --target-dir /tmp/dirsql-verify | grep -E '\bcsv\b|\bserde_yaml\b' \
  && echo 'FAIL: csv or serde_yaml still in tree' || echo 'OK: parser deps removed'

# 2. Confirm `format`/`each`/`[table.columns]` are silently ignored.
cat > /tmp/legacy.toml <<'TOML'
[[table]]
ddl  = "CREATE TABLE t (_path TEXT)"
glob = "*.json"
format = "json"
each   = "items"
[table.columns]
old = "metadata.name"
TOML
# Parses without error; the table produces filesystem-fact rows.

# 3. Confirm filesystem-fact auto-injection on a config-defined table.
mkdir -p /tmp/dirsql-fs/posts/abc
echo '{}' > /tmp/dirsql-fs/posts/abc/hello.md
cat > /tmp/dirsql-fs/.dirsql.toml <<'TOML'
[[table]]
ddl  = "CREATE TABLE posts (thread_id TEXT, _basename TEXT, _size INTEGER)"
glob = "posts/{thread_id}/*.md"
TOML
# A query of `SELECT thread_id, _basename, _size FROM posts` returns one
# row: ("abc", "hello.md", 3).
```

---

### `extract` callbacks no longer receive file content

#### Summary

The `extract` callback on a programmatic `Table` (Rust), `Table` (Python),
or `TableDef` (TypeScript) changed from a two-argument callback
`(path, content)` to a one-argument callback `(path)`. The single argument
is the **absolute filesystem path** of the matched file (previously the
first argument was the root-relative path). `dirsql` no longer reads file
bodies during the initial scan or the watch loop, so a callback that needs
file content must read it itself. Affects every consumer that registers a
programmatic table with an `extract` callback in any of the three SDKs.
Consumers who only use `.dirsql.toml` config files are unaffected — config
tables never had a user-authored `extract`. The change removes a vestigial
eager UTF-8 read left over from the content-parsing feature deleted in
[#169](https://github.com/thekevinscott/dirsql/issues/169); a side effect
is that a table glob may now match binary (non-UTF-8) files without
aborting the build. Closes part of
[#184](https://github.com/thekevinscott/dirsql/issues/184).

#### Required changes

| Surface | Before | After |
| ------- | ------ | ----- |
| Python `extract` (uses content) | `extract=lambda path, content: [json.loads(content)]` | `extract=lambda path: [json.loads(open(path, encoding="utf-8").read())]` |
| Python `extract` (ignores content) | `extract=lambda path, content: [...]` | `extract=lambda path: [...]` |
| Rust `extract` (uses content) | `Table::new(ddl, glob, \|_path, content\| parse(content))` | `Table::new(ddl, glob, \|path\| parse(&std::fs::read_to_string(path).unwrap()))` |
| Rust `extract` (ignores content) | `Table::new(ddl, glob, \|_path, _content\| ...)` | `Table::new(ddl, glob, \|_path\| ...)` |
| TypeScript `extract` (uses content) | `extract: (path, content) => [JSON.parse(content)]` | `extract: (path) => [JSON.parse(readFileSync(path, "utf8"))]` |
| TypeScript `extract` (ignores content) | `extract: (path, content) => [...]` | `extract: (path) => [...]` |
| Path argument semantics | first argument was the root-relative path | first (only) argument is the absolute filesystem path |

#### Deprecations removed

_None._ The two-argument signature was never deprecated; it is replaced in a
single release alongside the related zero-config work in #184.

#### Behavior changes without code changes

- A table glob that matches a binary / non-UTF-8 file no longer aborts
  construction. Previously `dirsql` eagerly read every matched file as UTF-8
  text and surfaced an `InvalidData` error; it now never reads file bodies
  itself, so binary files are indexed for their filesystem facts without
  error.
- The path handed to `extract` is now absolute rather than root-relative.
  Callbacks that derived columns from the path via `Path`/`os.path`
  component accessors (`parent`, `file_name`/`basename`) are unaffected;
  callbacks that compared the path against a hard-coded relative string must
  be updated.

#### Verification

```bash
# A programmatic table whose glob matches a binary file builds cleanly and
# the callback receives an absolute path it can open itself.
python - <<'PY'
import tempfile, os
from dirsql import DirSQL, Table

root = tempfile.mkdtemp()
open(os.path.join(root, "logo.png"), "wb").write(b"\xff\xd8\xff\x00")

db = DirSQL(root, tables=[Table(
    ddl="CREATE TABLE assets (_basename TEXT)",
    glob="*.png",
    extract=lambda path: (os.path.isabs(path) or 1/0) and [{}],
)])
import asyncio; asyncio.run(db.ready())
print(asyncio.run(db.query("SELECT _basename FROM assets")))
# expected: [{'_basename': 'logo.png'}]
PY
```

---

### Zero-config run serves a default `files` table

#### Summary

Running the `dirsql` server (no subcommand) in a directory without a
`.dirsql.toml` used to leave the server degraded: it bound the port but
every `POST /query` returned HTTP 503 with `config not found`. It now
indexes the directory with a built-in `files` table -- one row per file,
columns drawn entirely from filesystem facts -- and serves queries
normally. Affects only the CLI server's no-config path; consumers who
always run with a `.dirsql.toml`, and all programmatic SDK consumers, are
unaffected. Part of
[#184](https://github.com/thekevinscott/dirsql/issues/184).

#### Required changes

_None._ The change is additive for anyone who already ships a `.dirsql.toml`
-- a present config fully overrules the default.

#### Deprecations removed

_None._

#### Behavior changes without code changes

- `dirsql` started in a directory without a `.dirsql.toml`: previously every
  `POST /query` returned `503 Service Unavailable` with
  `{"error":"config not found at ./.dirsql.toml"}`; now the server is
  `Ready` and `POST /query` runs against a default `files` table (one row
  per file, columns `_path`, `_basename`, `_dir`, `_ext`, `_size`,
  `_mtime`, `_ctime`). Tooling that probed for the 503 to detect "no
  config" must instead check for the `files` table or for the presence of a
  `.dirsql.toml`. The 503 path still applies when a config file exists but
  fails to load.

#### Verification

```bash
cd "$(mktemp -d)"
echo hi > note.txt
dirsql --port 7117 &
sleep 1
curl -s localhost:7117/query -H 'content-type: application/json' \
  -d '{"sql":"SELECT _basename FROM files"}'
# expected: [{"_basename":"note.txt"}]
kill %1
```

---

### Release pipeline migrated to `putitoutthere`

#### Summary

The release process is now driven by [putitoutthere](https://github.com/thekevinscott/putitoutthere). No SDK call sites change; the migration is observable in tag layout, npm package layout, and CI configuration. Consumers installing via `pip install dirsql` / `cargo add dirsql` / `npm install dirsql` see no behavioral difference at install time. Operators reading release tags or pinning npm sub-packages by name need to update their references.

#### Required changes

| Surface | Before | After |
|---|---|---|
| Git tag for a release | one shared tag `v{version}` | three per-package tags `dirsql-rust-v{version}`, `dirsql-py-v{version}`, `dirsql-npm-v{version}` |
| npm CLI sub-packages | `@dirsql/cli-<short-slug>` (e.g. `@dirsql/cli-linux-x64-gnu`) | `@dirsql/cli-{triple}` (e.g. `@dirsql/cli-linux-x64-gnu`) — same scheme, retained via `name` template |
| npm napi sub-packages | `@dirsql/lib-<short-slug>` | `@dirsql/lib-{triple}` — same scheme, retained via `name` template |
| Release trigger | scheduled cron + immediate-on-push (toggle via `RELEASE_STRATEGY` repo var) | every push to `main` whose changes match a package's `globs` |
| Skip a release | `[no-release]` in commit message | `release: skip` trailer in commit body |
| Bump type | `workflow_dispatch` input (`patch` / `minor`) | `release: <bump>` trailer in commit body (default `patch`) |
| Publish auth | bootstrap `NPM_TOKEN` + `crates-io-auth-action` + PyPI TP | OIDC trusted publishers on all three registries; no long-lived tokens reachable from the workflow |

#### Deprecations removed

_None._

#### Behavior changes without code changes

- **PyPI wheels temporarily ship without the `dirsql` CLI binary.** The previous pipeline cross-compiled the Rust binary per target and bundled it into each wheel, so `pip install dirsql` shipped a working `dirsql` command. Putitoutthere v0.2.3 has the `[package.bundle_cli]` recipe in its schema but no workflow step that builds + stages the binary, so the block is dropped from `putitoutthere.toml` for now. `dirsql._cli.main` still installs as a Python module but raises `FileNotFoundError` if invoked, pointing at `cargo install dirsql --features cli` or `npx dirsql` as alternate install paths. To restore: re-add `[package.bundle_cli]` once the upstream gap is closed and `[project.scripts] dirsql = "dirsql._cli.main:main"` to `packages/python/pyproject.toml`.
- **Per-SDK selective publishing.** The `workflow_dispatch` `publish_python` / `publish_rust` / `publish_js` toggles are gone; package selection now flows through `release: <bump> [<pkg-name>, ...]` trailers (per-package names: `dirsql-rust`, `dirsql-py`, `dirsql-npm`).
- **Auto-rollback on partial publish failure** is no longer performed. The previous pipeline deleted the tag if both PyPI and crates.io publishes failed; under putitoutthere, a partial failure leaves the published artifacts in place and re-runs are idempotent (each handler's first move is `isPublished`, which short-circuits cleanly on already-published versions).
- **GitHub Release notes** are still auto-generated (`gh release create --generate-notes`) but the Release is now created by the reusable workflow, not the consumer's `publish.yml`.
- **Dry-run mode** is removed. The plan job is side-effect-free; inspect the matrix output on a feature branch to preview a release.

#### Verification

```
# 1. The new caller workflow lints clean.
yamllint .github/workflows/release.yml

# 2. The toml parses and the plan resolves.
#    (Locally — putitoutthere's `plan` is pure over (config + git state).)
npx -y putitoutthere@0.2 plan

# 3. Trusted publishers on all three registries point at this filename.
#    Expected entry on each:
#      Repository: thekevinscott/dirsql
#      Workflow:   release.yml
#      Environment: release
#    PyPI:    https://pypi.org/manage/project/dirsql/settings/publishing/
#    crates:  https://crates.io/crates/dirsql/settings
#    npm:     https://www.npmjs.com/package/dirsql/access
#             — plus one per per-platform package (see PR body).
```

<!--
When a PR introduces a breaking change, a deprecation removal, or a
behavior-only change, copy the template block below into the `## [Unreleased]`
section and fill it in. When a release is cut, rename `## [Unreleased]` to
`## [vX.Y.Z] - YYYY-MM-DD` and start a fresh Unreleased section above it.

Migration entries are required for:
  - Breaking API changes (signatures, names, return types, config keys)
  - Removal of a previously deprecated symbol
  - Behavior changes that keep the same API (exit codes, event payloads,
    on-disk layouts, default values, tag formats)

Migration entries are NOT required for purely additive changes, bug fixes that
restore documented behavior, or changes that are internal-only.
-->

---

## Migration entry template

Copy this block in full. Every subsection is required; if a subsection does
not apply, keep the heading and write `_None._`.

### `<Short title of the change>`

#### Summary

One paragraph. State what broke, which SDKs and call sites are affected, and
why the change was made (bug, parity, redesign, dependency upgrade). A reader
who lands here from a failing build should be able to decide in 30 seconds
whether this migration is the cause.

#### Required changes

A table of before/after snippets covering every affected surface: config
files, CLI flags, action inputs, function signatures, return types. One row
per distinct surface. Include per-SDK snippets where they differ.

| Surface | Before | After |
| ------- | ------ | ----- |
| `<e.g. Python DirSQL.open>` | `<prior call site>` | `<new call site>` |
| `<e.g. CLI flag>` | `<old flag>` | `<new flag>` |

#### Deprecations removed

Anything previously marked deprecated that is now gone. Consumers on the
prior version should have seen warnings; this section tells them which of
those warnings have become hard errors.

- `<deprecated symbol>` (deprecated in `<version>`) — removed; use `<replacement>`.

#### Behavior changes without code changes

Same API, different runtime behavior. Cover exit codes, tag/ID formats,
on-disk layouts, event payloads, retry behavior, default values. Each bullet
names the surface and describes the old vs. new behavior concretely.

- `<surface>`: previously `<old behavior>`; now `<new behavior>`. `<impact on
  consumer code, if any>`.

#### Verification

A concrete recipe a consumer can run to confirm the upgrade worked. Prefer a
dry-run or read-only command plus expected output; do not require them to
mutate real data.

```bash
<command>
# expected: <output>
```
