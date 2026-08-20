# Run all lints
lint:
    ruff check packages/python/

# Check formatting
format-check:
    ruff format --check packages/python/

# Auto-format
format:
    ruff format packages/python/

# Fix lint issues
fix:
    ruff check --fix packages/python/
    ruff format packages/python/

# Run Python unit tests (colocated)
test-unit:
    uv run python -m pytest packages/python/dirsql/ -x -q

# Run integration tests (hermetic: mocked core + fs)
test-integration:
    uv run python -m pytest packages/python/tests/integration/hermetic/ -x -q

# Run binding tests (real core, real fs)
test-binding:
    uv run python -m pytest packages/python/tests/integration/binding/ -x -q

# Run e2e tests (local only). Runs from packages/python so `uv run` resolves
# that package's own venv -- in a worktree, the worktree-local
# packages/python/.venv, not the shared repo-root .venv every worktree would
# otherwise trample (#682).
test-e2e:
    cd packages/python && uv run python -m pytest tests/e2e/ -x -q

# Run the Python packaging distcheck flow (build the wheel, install into a fresh
# venv, run the installed CLI) from the internals/distcheck package (#520). Runs in
# CI (python-test.yml `distcheck` job). Needs `cargo build -p dirsql --features cli`
# and `maturin` on PATH (uv sync in packages/python) first.
test-distcheck-python:
    uv run --project internals/distcheck dirsql-distcheck python

# Run the node packaging distcheck flow (pack -> npm install -> run) from the
# internals/distcheck package. Runs in CI (ts-test.yml `distcheck` job). Needs
# `pnpm build` in packages/ts first.
test-distcheck-node:
    uv run --project internals/distcheck dirsql-distcheck node

# Run internals/checks integration tests (real git, real pytest subprocess)
test-integration-internals-checks:
    cd internals/checks && uv run python -m pytest tests/integration -x -q

# Run internals/checks e2e tests (real `dirsql-checks` CLI subprocess, local only)
test-e2e-internals-checks:
    cd internals/checks && uv run python -m pytest tests/e2e -x -q

# Refresh internals/checks/e2e-attestation.json
e2e-attest-internals-checks:
    cd internals/checks && uvx testing-conventions e2e attest 'uv run python -m pytest tests/e2e -x -q'

# Refresh packages/python/e2e-attestation.json: runs the python e2e suite and
# commits the attestation. The CI gate runs inside the reusable workflow
# (conventions.yml, python-sdk `e2e-verify`) on PRs that touch the python SDK
# source. Install testing-conventions first (CI always uses the latest release):
#   pip install testing-conventions
e2e-attest-python:
    cd packages/python && testing-conventions e2e attest 'just test-e2e'

# Refresh packages/ts/e2e-attestation.json: runs the TS pack-install e2e suite
# and commits the attestation.
e2e-attest-ts:
    cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'

# Every testing-conventions gate CI declares, derived from the (source, gates)
# pairs of every workflow in .github/workflows that calls the reusable workflow
# -- 40 pairs across 8 scan roots in 6 workflows today (#781, #973). Supersedes
# the hand-restated `test-conventions` / `unit-coverage` / `mutation` /
# `e2e-verify` recipes, which named 6 pairs across 3 roots and drifted from the
# workflow. `--conventions` narrows it to named workflows (repeatable), `--gate`
# to named gates; `--dry-run` prints the matrix without running it. Needs the
# native build first for the suite-executing gates (maturin / napi); run
# `just build` if missing.
#
# `packaging` is reported SKIP, not run: it inspects a BUILT artifact, which CI
# builds from each manifest. Use `just test-packaging` for that locally.
#
# Each python root is first reconciled with its manifest (`uv sync`) and checked
# for undeclared imports (`declared-deps`, #782) -- a `uv pip install` leaves the
# venv strictly more capable than any real install, which no gate can detect.
preflight *ARGS:
    uv run --project internals/checks dirsql-checks preflight {{ARGS}}

# Packaging gate: assert no test files ship in the built .whl / .tgz / .crate.
# Mirrors the testing-conventions `packaging` gate run in conventions.yml;
# requires uv, pnpm, cargo, and `pip install testing-conventions`.
test-packaging:
    #!/usr/bin/env bash
    set -euo pipefail
    work="$(mktemp -d)"
    cd packages/python && uv run maturin build --out dist && cd ../..
    python3 -m zipfile -e packages/python/dist/*.whl "$work/wheel"
    testing-conventions packaging --language python "$work/wheel"
    cd packages/ts && pnpm build && mkdir -p dist-pack && pnpm pack --pack-destination dist-pack && cd ../..
    mkdir -p "$work/tgz" && tar -xzf packages/ts/dist-pack/*.tgz -C "$work/tgz"
    testing-conventions packaging --language typescript "$work/tgz/package"
    cargo package -p dirsql --no-verify --allow-dirty
    mkdir -p "$work/crate" && python3 -m tarfile -e target/package/*.crate "$work/crate"
    testing-conventions packaging --language rust "$work"/crate/dirsql-*

# CI test target (unit + integration + binding, no e2e)
# Imports the pytest-gate check's pure `run` directly via PYTHONPATH rather than
# the installed `dirsql-checks` entry point -- this must run under the ambient
# `packages/python` venv (which has dirsql + its test deps), not internals/checks'
# own venv (which has neither).
test-ci:
    PYTHONPATH=internals/checks/src uv run python -c "import sys; from checks.pytest_gate.gate import run; sys.exit(run(sys.argv[1:]))" packages/python/dirsql/ packages/python/tests/integration/ -x -q --tb=short

# Run Rust tests
test-rust:
    cargo test --workspace

# Run Rust clippy
clippy:
    cargo clippy --workspace -- -D warnings

# Run Rust format check
fmt-check:
    cargo fmt --all -- --check

# Full local CI
ci:
    just lint
    just format-check
    just clippy
    just fmt-check
    just test-rust
    just test-ci
    just preflight
