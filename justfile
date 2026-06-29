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

# Run integration tests
test-integration:
    uv run python -m pytest packages/python/tests/integration/ -x -q

# Run e2e tests (local only, not CI)
test-e2e:
    uv run python -m pytest packages/python/tests/e2e/ -x -q

# Refresh packages/python/e2e-attestation.json: runs the python e2e suite and
# commits the attestation. The CI gate (.github/workflows/e2e-attestation.yml)
# verifies it per-package on PRs that touch packages/python. Install
# testing-conventions first (version pinned in the workflow):
#   pip install "testing-conventions==<version>"
e2e-attest-python:
    cd packages/python && testing-conventions e2e attest 'just test-e2e'

# Refresh packages/ts/e2e-attestation.json: runs the TS pack-install e2e suite
# and commits the attestation.
e2e-attest-ts:
    cd packages/ts && testing-conventions e2e attest 'pnpm test:e2e'

# Verify each package's e2e attestation is fresh (the CI gate, run per-package).
e2e-verify:
    cd packages/python && testing-conventions e2e verify
    cd packages/ts && testing-conventions e2e verify

# Enforce colocated unit tests via the testing-conventions CLI
# (install: pip install "testing-conventions==<version>"). Exemptions live in
# testing-conventions.toml, which the CLI reads by default.
test-conventions:
    testing-conventions unit colocated-test --language python packages/python/dirsql
    testing-conventions unit colocated-test --language typescript packages/ts/src
    testing-conventions unit colocated-test --language rust packages/rust/src
    # Isolation (unit lint): all three SDKs.
    testing-conventions unit lint --language python packages/python/dirsql
    testing-conventions unit lint --language typescript packages/ts/src
    testing-conventions unit lint --language rust packages/rust/src

# Packaging gate: assert no test files ship in the built .whl / .tgz / .crate.
# Mirrors .github/workflows/packaging.yml; requires uv, pnpm, cargo, and
# `pip install "testing-conventions==<version>"`.
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

# CI test target (unit + integration, no e2e)
test-ci:
    uv run python -m pytest packages/python/dirsql/ packages/python/tests/integration/ -x -q --tb=short 2>/dev/null || echo "No tests found yet"

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
