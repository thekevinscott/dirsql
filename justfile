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

# Verify each package's e2e attestation is fresh. Mirrors the CI gate, which now
# runs inside conventions.yml (python-sdk / typescript-sdk `e2e-verify`).
e2e-verify:
    cd packages/python && testing-conventions e2e verify
    cd packages/ts && testing-conventions e2e verify
    cd internals/checks && testing-conventions e2e verify

# Enforce colocated unit tests via the testing-conventions CLI
# (install: pip install testing-conventions). Exemptions live in
# testing-conventions.toml, which the CLI reads by default.
test-conventions:
    testing-conventions unit colocated-test --language python packages/python/dirsql
    testing-conventions unit colocated-test --language typescript packages/ts/src
    testing-conventions unit colocated-test --language rust packages/rust/src
    # Isolation (unit lint): all three SDKs.
    testing-conventions unit lint --language python packages/python/dirsql
    testing-conventions unit lint --language typescript packages/ts/src
    testing-conventions unit lint --language rust packages/rust/src

# Enforce the unit-only coverage floor via testing-conventions (#234/#295).
# Floors live in testing-conventions.toml ([python|typescript|rust].coverage).
# Needs the native build first (maturin / napi); run `just build` if missing.
# Rust's branch floor runs `cargo llvm-cov --lib --features cli --branch`, which
# needs a nightly toolchain + llvm-tools-preview as the active toolchain.
unit-coverage:
    cd packages/python && uv run testing-conventions unit coverage --language python --config ../../testing-conventions.toml dirsql
    cd packages/ts && testing-conventions unit coverage --language typescript --config ../../testing-conventions.toml src
    testing-conventions unit coverage --language rust --config testing-conventions.toml packages/rust/src

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
    just test-conventions
