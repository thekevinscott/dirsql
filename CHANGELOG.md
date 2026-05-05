# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

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
