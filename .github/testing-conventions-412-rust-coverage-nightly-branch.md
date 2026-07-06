# unit-coverage: provision the nightly rust toolchain for branch coverage, so rust `unit-coverage` can run in the reusable workflow

_Paste-ready issue for `thekevinscott/testing-conventions`. dirsql's session GitHub scope is locked to `thekevinscott/dirsql`, so it could not be filed from there directly._

## Summary

The reusable `unit-coverage` job now installs/builds from the derived package root and runs for **python and typescript** (#284). It does **not** yet run for **rust**, for one concrete reason: rust *branch* coverage (`cargo llvm-cov --branch`) is **nightly-only**, and the reusable coverage job provisions a stable toolchain. The CLI side is already done — `unit coverage --language rust` scopes to `--lib` (#269), reads `[rust].features` (#270), and enforces the `[rust.coverage]` `lines/functions/regions/branch` floors (#271). The only gap is that the reusable *job* has no way to provision the nightly toolchain (+ `llvm-tools-preview` + `cargo-llvm-cov`) that `--branch` requires. So rust coverage stays bespoke while python/ts are adopted.

## Concrete shape (dirsql's bespoke rust coverage job — what we want to delete)

`rust-test.yml` `coverage` job:

```yaml
- name: Install Rust
  uses: dtolnay/rust-toolchain@master
  with:
    toolchain: nightly-2026-05-28          # pinned nightly — branch cov is nightly-only
    components: llvm-tools-preview
- name: Install cargo-llvm-cov
  uses: taiki-e/install-action@cargo-llvm-cov
- name: Install Node
  uses: actions/setup-node@v4
  with: { node-version: "24" }
- name: Unit coverage floor (testing-conventions)
  run: npx -y testing-conventions unit coverage --language rust packages/rust/src
```

The CLI shells `cargo llvm-cov --lib --features cli --branch`. Everything here except **the nightly toolchain + `llvm-tools-preview` + `cargo-llvm-cov` provisioning** is already inside the reusable job for python/ts.

## Why the existing #284 install path isn't enough

`#284` provisions the *stable* toolchain (via `provision_rust`) for building native artifacts. Branch coverage needs a *nightly* toolchain, `llvm-tools-preview`, and `cargo-llvm-cov` — a distinct provisioning path from "build the crate on stable." Without it the reusable rust coverage job either can't compute `branch` or fails on the nightly-only flag.

## The ask

Teach the reusable `unit-coverage` job to provision the rust branch-coverage toolchain when the language is rust and `[rust.coverage].branch = true`:

1. Install a **nightly** rust toolchain with `llvm-tools-preview` (not stable).
2. Install `cargo-llvm-cov`.
3. Run `unit coverage --language rust <src>` as today.

Because branch coverage on nightly is version-sensitive, let the caller **pin the nightly toolchain** (e.g. a `rust_toolchain_version` input, or a `[rust.coverage].toolchain` config key) defaulting to a floating `nightly`. When `branch = false`, stable is fine and no nightly is needed.

Secondary (already how we run it, so worth matching): rust coverage should be **whole-tree only** — no `--base` changed-lines check. Rust's effectful paths (filesystem / subprocess / HTTP / `notify`) are unit-uncovered by design, so a per-PR changed-lines floor would fail legitimate integration-tier edits. The python/ts jobs run both whole-tree and `--base`; rust should be able to opt out of the `--base` half.

## Acceptance

- A consumer adds `"unit-coverage"` to a rust package's `gates` (with `[rust.coverage]` floors incl. `branch`), and the reusable job provisions nightly + `llvm-tools-preview` + `cargo-llvm-cov`, runs `unit coverage --language rust`, and enforces the floors — no bespoke coverage job.
- The nightly toolchain is pinnable by the caller.
- Rust coverage runs whole-tree only (no `--base`).
- dirsql retires `rust-test.yml`'s `coverage` job and adopts rust `unit-coverage` into `conventions.yml`.

## Environment

Reusable workflow `@v0` (`174550e7`). CLI rust-coverage capability (`--lib` scope, `features` passthrough, floors) already shipped (#269/#270/#271); this is purely the reusable job's nightly-toolchain provisioning for `--branch`. Consumer: dirsql `main`, `packages/rust/src`, floors `[rust.coverage]` lines 94 / functions 91 / regions 93 / branch 75, nightly pinned `nightly-2026-05-28`.
