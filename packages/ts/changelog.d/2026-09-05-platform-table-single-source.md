**Changed** — the published-target table moved out of `src/platforms.ts` into
`src/platforms.json`, the one declarative copy of the data. `platforms.ts` types
and re-exports it; the node distcheck flow reads the same file instead of
hand-maintaining a Python subset, so the `platforms-mirror` check that policed
the two copies is gone. `PLATFORMS`, `libTriples()` and `librarySlug()` keep
their shapes, and the JSON ships in `dist/` alongside the compiled modules.
