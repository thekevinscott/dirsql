# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Python SDK ships PEP 561 type information.** The wheel now bundles
  `dirsql/py.typed` and `dirsql/_dirsql.pyi`, so downstream consumers see
  types for `DirSQL`, `Table`, `RowEvent`, and `__version__` in editors and
  type checkers. The stub is hand-written and mirrors the PyO3 bindings in
  `packages/python/src/lib.rs`; any change to that source's public surface
  must update the stub in the same PR. Parity-restoring with the Rust core
  (types from source) and TypeScript SDK (types from generated `.d.ts`).

- **Strict type-checking in CI for the Python SDK (`ty`).** Adds
  `.github/workflows/python-typecheck.yml` running Astral's `ty` with every
  rule promoted to `"error"` (`--error all --error-on-warning`). Existing
  untyped code is frozen behind line-precise `# ty:ignore[<rule>]` baseline
  comments inserted by `ty check --add-ignore` -- effectively a TODO list
  that shrinks as files are touched. New errors in new or edited code fail
  CI immediately. Ruff `PGH003` is enabled to forbid bare `# type: ignore`
  / `# ty: ignore` without a rule code. Test files (`**/test_*.py`,
  `**/*_test.py`) are excluded for now because their `monkeypatch.setattr`
  fakes are a separately-tracked smell (AGENTS.md "Test Boundaries").

- **Dedicated TypeScript type-check job.** Adds
  `.github/workflows/ts-typecheck.yml` running `tsc --noEmit` standalone
  on every PR touching `packages/ts/**/*.ts`. `tsc` was already running
  inside the `ts-test.yml` build step (`tsconfig.json` has `strict`,
  `noUncheckedIndexedAccess`, `verbatimModuleSyntax`), but a dedicated
  job surfaces type errors in seconds without waiting on `napi build` --
  the native addon is loaded via `createRequire` at runtime, so type
  checking does not need `dirsql.node` or the generated `index.d.ts`.
  Parity with the new Python type-check job.

- **`dirsql interpret <config>` subcommand.** Long-running NDJSON
  helper that loads a native-language config file (Python `.py`,
  TypeScript / JavaScript `.js` / `.mjs` / `.cjs`), takes its
  `app` / default export, writes a single handshake line
  (`{"type": "config", "state": <vars(app)> | <app.toJSON()>}`),
  then loops on stdin handling `{"type": "extract", "id", "table", "path"}`
  requests by dispatching to the config's user-defined `extract`
  callbacks. One request / one response, sequential. Used by the
  forthcoming Rust orchestrator; also directly invokable for
  debugging native configs. Python and TypeScript only — Rust has no
  host language runtime in which user callbacks could execute
  (intentional parity drift). The PyO3 `Table` class now exposes
  `extract` and `name` as readable attributes; `name` comes from the
  core `dirsql::db::parse_table_name`. The TypeScript SDK gains a
  top-level `parseTableName(ddl)` export backed by the same Rust
  function, and `DirSQL._options` is now public-readable so the
  TypeScript dispatcher can reach the original `TableDef` (which
  carries the `extract` closure that `toJSON()` intentionally drops).
  (#196)

- **Config serialization on `DirSQL`.** Python `vars(db)` (via
  `__dict__`), TypeScript `JSON.stringify(db)` (via `toJSON()`), and
  Rust `db.config()` (returning a `serde::Serialize`-derived
  `DirSQLConfig`) all return the resolved construction state as a
  JSON-compatible value with fields `root`, `tables`, `ignore`,
  `persist`, `persist_path` (camelCase `persistPath` in TypeScript).
  Each table is `{ ddl, glob, strict }`. The original `config` path is
  excluded (already merged into `root` / `tables` / `ignore`);
  per-table `extract` and `name` are excluded. Resolution runs
  synchronously in Python and TypeScript, so the output is available
  immediately after construction without waiting on the scan. (#194)

- **Zero-config `files` table.** Running the `dirsql` server in a
  directory with no `.dirsql.toml` now serves a default `files` table --
  one row per file under the directory, with the filesystem-fact columns
  `_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime` --
  instead of starting in the degraded (HTTP 503) state. `SELECT * FROM
  files` and `SELECT name FROM sqlite_master` work immediately in any
  directory; no ignores are applied, so every file is indexed. A
  `.dirsql.toml`, when present, fully overrules the default. (#184)

### Changed

- **CLI launcher coverage exclude lifted; launchers unit-tested.** The
  Python `dirsql.cli.main` / `binary_path` / `is_windows` and the
  TypeScript `main` / `die` / `resolveBinary` helpers are now fully
  unit-tested. The coverage configs no longer omit the launcher
  directories wholesale -- only the `dirsql.ts` npm `bin` shim stays
  excluded. Tests fake out the launchers' module imports and process
  state via `unittest.mock.patch.object` (Python) and `vi.mock` /
  `vi.stubGlobal` (TypeScript); the launcher production signatures
  are unchanged. No user-facing behavior change. (#211)

- **CLI launcher directories renamed for cross-SDK consistency.** The
  Python `dirsql/_cli/` package and the TypeScript `src/bin/` directory
  are both renamed to `cli/`. The Python `[project.scripts]` entry-point
  moves from `dirsql._cli.main:main` to `dirsql.cli.main:main`; the npm
  `bin` field points at `dist/cli/dirsql.js` instead of
  `dist/bin/dirsql.js`. The user-facing `dirsql` command on PATH is
  unchanged. The Python leading underscore was misleading (the directory
  holds the public console-script entry, not internal-only code), and
  the TypeScript `bin/` vs. Python `_cli/` mismatch made cross-SDK
  references awkward. See `MIGRATIONS.md`. (#210)

- **`extract` callbacks no longer receive file content.** The `extract`
  callback on a programmatic `Table` (Rust/Python) / `TableDef`
  (TypeScript) now takes a single argument — the absolute filesystem
  path of the matched file — instead of `(path, content)`. `dirsql` no
  longer reads file bodies during the scan or watch loop; a callback
  that needs the file content reads it itself (`open(path)` /
  `std::fs::read_to_string(path)` / `readFileSync(path)`). This removes
  a vestigial eager UTF-8 read left over from the `format`/`each`
  config grammar deleted in #169, and lets a table glob match binary
  files without aborting the build. Breaking change across the Python,
  Rust, and TypeScript SDKs; `.dirsql.toml` config users are
  unaffected. See `MIGRATIONS.md`. (#184)

### Removed

- **Python 3.10 support dropped.** `requires-python` is now `>=3.11`
  (was `>=3.10`). `pip` / `uv` will refuse to install dirsql on Python
  3.10. This is required to ship 0.3.6: putitoutthere's multi-version
  wheel build (#369) fans a wheel row per `requires-python` version,
  and its `bundle_cli` wheel-content verify step `import tomllib` —
  stdlib only on CPython >= 3.11 — so the 3.10 row crashes the release
  build. Dropping 3.10 removes that row. 3.10 can be restored once the
  upstream verify step no longer depends on `tomllib`.

### Fixed

- **`npx dirsql` works on Linux hosts with glibc < 2.39 again.** The
  npm `bundled-cli` Linux binary in 0.3.11 / 0.3.12 was dynamically
  linked against `GLIBC_2.39` and crashed at startup on Ubuntu 22.04,
  Debian 12, Amazon Linux 2, and any other host whose runtime glibc
  predates Ubuntu 24.04's. Root cause was a missing
  `[package.bundle_cli]` subtable on the `dirsql-npm` package in
  `putitoutthere.toml`: every musl / cross-compile / stage / verify
  step in upstream `_matrix.yml`'s npm bundled-cli row is gated on
  `matrix.bundle_cli`, so without the subtable nothing in the upstream
  pipeline produced a binary, and the consumer's local
  `npm run build` (`packages/ts/tools/stagePlatform.ts`) silently fell
  through to plain `cargo build --target x86_64-unknown-linux-gnu` on
  the ubuntu-latest runner. Adding the subtable (mirroring the pypi
  block) makes upstream cross-compile against the musl triple and stage
  the static binary into `packages/ts/build/bundled-cli-{triple}/` after
  `npm run build` (putitoutthere#386 ordering fix), so the static binary
  is what reaches the upload-artifact step. A new vitest in
  `packages/ts/tools/putitoutthereConfig.test.ts` keeps the invariant
  honest: any future package that declares
  `build = [..., { mode = "bundled-cli", ... }, ...]` must also declare
  the matching `[package.bundle_cli]` subtable. (#189)

- **`npx dirsql` and `uvx dirsql` work end-to-end again.** 0.3.5 published
  but the CLIs were still broken: the npm-bundled binary was packed
  without the executable bit (`spawnSync ... EACCES`) and stamped with a
  stale version, and PyPI shipped a cp312-only wheel so non-3.12
  interpreters fell back to a sdist build with no bundled binary. All
  three are fixed upstream in putitoutthere (`bundled-cli` now `chmod
  +x`es the staged binary and rewrites its version; `pypi` builds one
  wheel per `requires-python` version). dirsql consumes the fixes via
  `release.yml`'s `@v0` pin; 0.3.6 is the first release built by the
  corrected pipeline.
- **Release pipeline no longer fails publishing the crate to crates.io.**
  Removed the `packages/rust/target` symlink (and its root `.gitignore`
  exception) that worked around putitoutthere's `bundle_cli`
  workspace target-dir lookup. `cargo publish` followed the tracked
  symlink and archived the entire workspace build tree, producing a
  ~133 MiB `.crate` that crates.io rejected with `413 Payload Too
  Large` (10 MiB cap) -- failing the `release / publish` job and
  blocking every release after 0.3.4. putitoutthere #337 fixes the
  underlying workspace lookup, so the symlink (and its workarounds) are
  no longer needed.
- **Python wheel ships the `dirsql` CLI again.** Restored
  `[project.scripts] dirsql = "dirsql._cli.main:main"` in
  `packages/python/pyproject.toml` and declared `[package.bundle_cli]`
  in `putitoutthere.toml` (requires putitoutthere ≥ v0.2.17). The
  reusable workflow now cross-compiles the `dirsql` bin per target with
  `--features cli`, stages it into `dirsql/_binary/`, and
  maturin's `[tool.maturin].include` glob bundles it into each wheel.
  `pip install dirsql && dirsql ...` and `uvx dirsql` work end-to-end;
  upstream verifies wheel contents post-build.

### Added

- **PR-time config-sanity gate.** New
  `.github/workflows/release-config-check.yml` calls putitoutthere's
  `check.yml@v0` reusable workflow on every pull request. Validates
  `putitoutthere.toml` (parse + schema + common-mistakes detector,
  unique package names, `depends_on` cycle / dangling-ref detection,
  glob coverage, tag-format collisions), npm `repository` field, crates
  `description` / `license`, pypi `bundle_cli` binary declaration, and
  npm target triple mapping. Few seconds per PR, no per-target build.
  Companion to the existing `release-precheck.yml` build-matrix gate.

- **`pack-install` build-CI job for the npm package.** Builds the real
  `dirsql` binary, packs the host's `@dirsql/cli-<slug>` sub-package and
  the main `dirsql` package, installs both into a fresh dir, and runs
  `node_modules/.bin/dirsql --version`. Companion to the Python
  wheel-install job; gates publishability of the npm artifact.
- **AGENTS.md test-boundary rules.** Integration tests target the
  public SDK API (not the CLI launcher); e2e tests have no mocks /
  fakes / monkeypatching; monkeypatching production-module attributes
  is a code smell and is not allowed (use dependency injection via the
  public API or fixtures).

### Removed

- **Monkeypatch unit tests for the CLI launcher.** Deleted
  `packages/python/python/dirsql/_cli/{main,binary_path,is_windows}_test.py`,
  `packages/ts/ts/bin/{main,resolveBinary,die}.test.ts`, and
  `packages/ts/test/{cli.test.ts,runLauncher.ts,fakeInstallRoot.ts}`. The
  launchers are thin enough that the only meaningful coverage is the
  build-CI smoke tests above; the deleted tests monkeypatched production
  module attributes (`os.execv`, `process.exit`, `process.stderr`,
  `binary_path`, `resolveBinary`) which the new test-boundary rules
  forbid.
- **Content parsing is no longer a dirsql concern.** `parser.rs` and the
  `Format` enum (`Json` / `Jsonl` / `Csv` / `Tsv` / `Toml` / `Yaml` /
  `Frontmatter`) are gone, along with `ColumnSource`, `apply_columns`,
  `parse_file`, `infer_format`, and every `parse_*` / `navigate_*` helper.
  The corresponding `format`, `each`, and `[table.columns]` keys in
  `.dirsql.toml` are no longer recognized as part of the grammar (they parse
  but are ignored). The `csv` and `serde_yaml` Cargo dependencies are
  dropped. Closes #169.
- `DirSqlError::NoFormat` and `ConfigError::UnknownFormat` variants.

### Changed

- **`[[table]]` entries in `.dirsql.toml` now produce filesystem-fact rows
  instead of parsed-content rows.** Each matched file emits one row; columns
  come from glob path captures (named `{placeholder}` segments) and reserved
  stat virtuals (`_path`, `_basename`, `_dir`, `_ext`, `_size`, `_mtime`,
  `_ctime`). The DDL declares which subset of these the SQL table exposes;
  undeclared keys are silently dropped during normalization (in non-strict
  mode) or rejected (in strict mode).
- **All tables — programmatic and config-driven — now auto-inject glob
  captures and stat virtuals into every row** (filtered to the DDL's
  declared columns). User-extract values win over auto-injected values when
  keys collide. Programmatic `Table::new(...)` users no longer need to parse
  the path themselves to surface capture columns or stat metadata.
- `ARCHITECTURE.md` now opens with an explicit scope statement — dirsql is
  a queryable index over a filesystem; content interpretation is out of
  scope.

### Added

- **`dirsql init` subcommand.** Generates a starter `.dirsql.toml` for a
  directory by shelling out to the local `claude` CLI. Filesystem-fact
  only: emits `[[table]]` blocks whose columns come from glob path
  captures and reserved stat virtuals (`_path`, `_basename`, `_dir`,
  `_ext`, `_size`, `_mtime`, `_ctime`). Bails before invoking `claude`
  if the output already exists; `--force` overrides. Refuses to run
  with a clear error if `claude` is not on `PATH`. Flags: `--root`,
  `--output`, `--force`. See `docs/guide/init.md`. Closes #96.

- **Persistent on-disk SQLite cache.** Opt-in `persist` / `persist_path`
  option on `DirSQL` (and `[dirsql] persist = true` in
  `.dirsql.toml`). When enabled, the database is written to
  `<root>/.dirsql/cache.db` (override via `persist_path`) so
  subsequent startups only re-parse files that have actually changed.
  Reconcile uses size + mtime when the mtime is safely outside the
  racy window, falling back to a BLAKE3 content hash otherwise.
  Glob/DDL changes and a `dirsql_version` bump force a full rebuild,
  so SQL state never silently disagrees with the filesystem after
  reconcile. `.dirsql/` is unconditionally excluded from the scan
  walk. Available across all three SDKs (Rust `DirSQL::builder().persist(..)`,
  Python `DirSQL(..., persist=True)`, TypeScript `new DirSQL({ ..., persist: true })`).
  Closes #95.

### Fixed

- **PyPI wheels now ship at the planned release version.** Previously
  every wheel shipped with `0.1.0` baked in regardless of what the
  release plan computed, because `packages/python/pyproject.toml`
  declared a static `version = "0.1.0"` literal and maturin reads that
  field verbatim with no env override. Switched to maturin's
  dynamic-version mode (`dynamic = ["version"]` in `pyproject.toml`,
  with the literal moved to `packages/python/Cargo.toml`'s
  `[package].version`). Putitoutthere's `write-version` step
  (added upstream in thekevinscott/putitoutthere#277) rewrites the
  Cargo.toml literal before `maturin build`, so wheels ship at the
  planned version. Closes #166.

### Changed

- **Release pipeline rewritten on top of [putitoutthere](https://github.com/thekevinscott/putitoutthere).**
  The hand-rolled `patch-release.yml` + `publish.yml` + `publish-npm.yml`
  + `release-scripts.yml` + cargo-dist `release.yml` stack is replaced
  with a single `.github/workflows/release.yml` that calls the reusable
  `thekevinscott/putitoutthere/.github/workflows/release.yml@v0` workflow.
  Configuration moves to `putitoutthere.toml` at the repo root. Auth is
  OIDC trusted publishers on all three registries — no long-lived
  registry tokens. Per-package tags replace the single shared `v{version}`
  tag (each of `dirsql-rust`, `dirsql-py`, `dirsql-npm` now tags as
  `<name>-v<version>`); historical `v0.2.x` tags remain untouched. See
  [MIGRATIONS.md](./MIGRATIONS.md) for the consumer-visible details.
- **npm build tooling collapsed to a single per-host staging script.**
  `packages/ts/tools/stagePlatform.ts` runs `napi build --release` and
  `cargo build --release --bin dirsql --features cli --target <triple>`
  for the host triple, then stages outputs at
  `packages/ts/build/napi-{triple}/` and
  `packages/ts/build/bundled-cli-{triple}/` for putitoutthere's
  npm-platform handler to package. Replaces the cargo-dist-driven
  `buildPlatforms.ts` / `buildLibPlatforms.ts` / `buildOne.ts` /
  `buildLibOne.ts` / `extract.ts` / `findBinary.ts` /
  `syncVersion.ts` cross-compile pipeline; each putitoutthere matrix
  row now runs on a runner native to its target, so cross-compilation
  is no longer needed.

### Added

- **`.github/workflows/bootstrap-npm-platforms.yml`.** One-time
  manually-dispatched workflow that publishes `0.0.0-bootstrap` stubs
  for the ten per-platform sub-packages (`@dirsql/lib-*` and
  `@dirsql/cli-*`) using a long-lived `NPM_TOKEN`. Required because
  npm's trusted-publisher feature can't be enabled on a package that
  doesn't yet exist, and putitoutthere v0.2 deliberately doesn't pass
  long-lived tokens through the reusable workflow. Delete the workflow
  + secret after bootstrap completes and the trusted publishers are
  registered.

### Removed

- **PyPI wheels no longer ship the `dirsql` CLI binary.** Putitoutthere
  v0.2.3's `[package.bundle_cli]` recipe is parsed but its reusable
  workflow has no step that cross-compiles + stages the binary, so
  declaring the block would silently produce wheels missing the binary.
  Block is dropped from `putitoutthere.toml` (and the matching
  `[project.scripts] dirsql` entry from `pyproject.toml`) until the
  upstream gap closes. CLI install paths during this window:
  `cargo install dirsql --features cli` or `npx dirsql`.
- `scripts/release/` (custom Python orchestration: `compute_version.py`,
  `check_published.py`, `resolve_publish_targets.py`, plus their tests
  and `pyproject.toml`). Functionally replaced by putitoutthere's
  built-in cascade detection, version computation, and `isPublished`
  pre-check.
- `dist-workspace.toml` and the orphaned `[profile.dist]` block in the
  workspace `Cargo.toml`. cargo-dist no longer cuts the release.
- `.github/workflows/patch-release.yml`, `.github/workflows/publish.yml`,
  `.github/workflows/publish-npm.yml`, `.github/workflows/release-scripts.yml`.

### Added

- `dirsql` CLI binary (Rust, `--features cli`). Running the binary
  starts a long-lived HTTP server bound to `localhost:7117` that
  exposes the SDK over the network:
  - `POST /query` — JSON-in, JSON-rows-out.
  - `GET /events` — Server-Sent Events stream of row change events;
    payloads mirror `DirSQL::watch()`'s `RowEvent`.
  - Graceful shutdown on `SIGINT` / `SIGTERM` that drains in-flight
    requests and closes any attached SSE streams.
- Opt-in `cli` Cargo feature. Library consumers (`cargo add dirsql`)
  pull zero CLI dependencies; `cargo install dirsql --features cli`
  builds the binary.
- Distribution scaffolding:
  - `cargo-dist` config (`dist-workspace.toml` + auto-generated
    `.github/workflows/release.yml`) producing per-target archives on
    every `v*.*.*` tag.
  - Per-platform npm sub-packages published under `@dirsql/cli-*`,
    driven by `packages/ts/tools/buildPlatforms.ts` from the cargo-dist
    archives. Main `dirsql` npm package gains a `bin/dirsql.js`
    launcher and an `optionalDependencies` list that picks the right
    sub-package at install time (esbuild/biome/swc pattern).
  - PyPI wheels bundle the Rust binary directly: the
    `.github/workflows/publish.yml` build job stages
    `packages/python/python/dirsql/_binary/` before `maturin build`,
    and `[tool.maturin] include` ships it as package data. The new
    `dirsql._cli.main:main` console-script execs it. Pip wheel tags
    handle the platform dispatch.
- Documentation:
  - `docs/guide/cli.md` — HTTP server, flags, endpoints, SSE schema,
    "why SSE" rationale.
  - `packages/rust/README.md` now distinguishes the library install
    from the opt-in CLI install and calls out the `required-features`
    silent-skip footgun.
  - `CHANGELOG.md` (this file).
  - Three-tier docs structure: root `README.md` mirrors the layout of
    `docs/` (one `##` per page) so agents reading the source can
    navigate without leaving the repo, every page in `docs/` has a
    `canonical` frontmatter + visible link back to its published URL,
    and the `docs/` folder now ships inside each published SDK
    package (`packages/<lang>/docs` symlinks the workspace `docs/`;
    cargo, maturin, and the npm `prepack` staging script include the
    markdown content in published artifacts).
- Tests:
  - 11 in-process HTTP integration tests
    (`packages/rust/tests/cli_integration.rs`) covering every
    documented endpoint, error class, method mismatch, and graceful
    shutdown.
  - 9 e2e tests (`packages/rust/tests/cli_e2e.rs`) that spawn the
    compiled binary and drive it over real HTTP / SSE / filesystem
    mutations.
  - Full TypeScript unit-test coverage (16 cases) for the npm launcher
    + build tooling, vitest-reported 100% lines / 95%+ branches /
    100% functions.
  - Python launcher tests migrated to `pytest_describe` blocks.

### Notes for maintainers

- Required repo secrets for the first tagged release:
  - `NPM_TOKEN` (publishes `dirsql` and `@dirsql/cli-*`).
  - `PYPI_API_TOKEN` (already used by `publish.yml`; trusted publisher
    works too).
- `@dirsql` npm scope must exist and be owned by the release account.
