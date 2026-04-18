# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
