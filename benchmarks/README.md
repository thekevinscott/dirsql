# Benchmarks

## Rust benchmarks (criterion)

Run all benchmarks:

```bash
cargo bench -p dirsql-core
```

Run a specific benchmark:

```bash
cargo bench -p dirsql-core --bench db_bench
cargo bench -p dirsql-core --bench scanner_bench
cargo bench -p dirsql-core --bench differ_bench
cargo bench -p dirsql-core --bench matcher_bench
```

Results are written to `packages/core/target/criterion/` with HTML reports.

### What is benchmarked

- **db_bench** -- SQLite operations: table creation, row insertion (1/100/1000 rows), query performance, delete-by-file
- **scanner_bench** -- Directory scanning: walk a temp directory with N files, matching against glob patterns
- **differ_bench** -- Row diffing: compare old/new row sets (no-change, single-line change, append, full replace) at various sizes
- **matcher_bench** -- Glob matching: match files against N table patterns, ignore-pattern checking, matcher construction cost

## Python benchmarks

Not yet implemented. Plan: use pytest-benchmark to measure Python SDK overhead on top of the Rust core.
