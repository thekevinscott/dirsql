**Fixed** — the published `.crate` archive no longer ships `changelog.d/` and
`migrations.d/`. `[package].exclude` named `tests/` and the VitePress tooling
under `docs/` but not the fragment dirs, and cargo ships whatever is not
excluded, so every release carried 60 fragment files no crate consumer reads —
more than half the archive's entries (119 files → 59).

No API, CLI or behavior change; the crate is smaller and its file list now
matches the npm package (an allowlist) and the wheel (maturin's layout), which
already dropped their fragments structurally.
