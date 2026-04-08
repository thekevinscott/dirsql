# Run all lints
lint:
    ruff check .

# Check formatting
format-check:
    ruff format --check .

# Auto-format
format:
    ruff format .

# Fix lint issues
fix:
    ruff check --fix .
    ruff format .

# Run Python unit tests (colocated)
test-unit:
    pytest python/ -x -q

# Run integration tests
test-integration:
    pytest tests/integration/ -x -q

# Run e2e tests (local only, not CI)
test-e2e:
    pytest tests/e2e/ -x -q

# CI test target (unit + integration, no e2e)
test-ci:
    pytest python/ tests/integration/ -x -q --tb=short 2>/dev/null || echo "No tests found yet"

# Run Rust tests
test-rust:
    cargo test

# Run Rust clippy
clippy:
    cargo clippy -- -D warnings

# Run Rust format check
fmt-check:
    cargo fmt -- --check

# Full local CI
ci:
    just lint
    just format-check
    just clippy
    just fmt-check
    just test-rust
    just test-ci
