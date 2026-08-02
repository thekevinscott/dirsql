**Changed** — release builds now strip symbols, run fat LTO, and use a single
codegen unit, so the published `dirsql` binary is ~33% smaller (linux-x64:
9.17 MB → 6.14 MB; 3.49 MB → 2.80 MB gzipped).

Build-only: no API, CLI or behavior change. `panic` stays at the default
`unwind`, which the pyo3 and napi bindings need to turn Rust panics into
Python/JS exceptions. Release build time rises ~2m10s → ~2m38s.
