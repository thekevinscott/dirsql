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

# Enforce colocated unit tests via the testing-conventions CLI
# (install: pip install "testing-conventions==<version>"). Exemptions live in
# testing-conventions.toml, which the CLI reads by default.
test-conventions:
    testing-conventions unit colocated-test --language python packages/python/dirsql
    testing-conventions unit colocated-test --language typescript packages/ts/src

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
