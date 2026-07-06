# packaging: auto-provision the dist build in the reusable job (like coverage/mutation), so native monorepos don't need a bespoke build job

_Paste-ready issue for `thekevinscott/testing-conventions`. dirsql's session GitHub scope is locked to `thekevinscott/dirsql`, so it could not be filed from there directly._

## Summary

The reusable `packaging` job is the **only** build-dependent gate that does not build its own input. Coverage and mutation were taught to self-provision the native build from the derived package root (#277/#279/#284 — maturin via `uv sync`, napi auto-provision, cargo self-provision), so a consumer adopts them by adding a gate name to an existing per-package call — no new jobs. `packaging` instead only consumes a caller-uploaded `packaging_artifact`. For a native-building monorepo that means **reintroducing a bespoke build job** (maturin build + `pnpm pack` + `cargo package` + `upload-artifact`) just to feed the gate — duplicating the very build the gate is supposed to subsume, and diverging from the "reusable does the work, caller stays thin" model the other gates now follow.

## Proven on dirsql (probe)

Adopting `packaging` required a ~90-line `build-dists` caller job plus a dedicated reusable call with `packaging_artifact`. It went green — the gate works — but the complexity is the point: every other gate adoption was a one-line allowlist edit, this one was a whole new job that hand-duplicates `packaging.yml`'s builds.

## The asymmetry

| Gate | Reusable builds the input? | Consumer adoption |
|------|----------------------------|-------------------|
| unit-coverage | **Yes** — provisions from `path` manifest (#284) | add `"unit-coverage"` to `gates` |
| mutation | **Yes** — self-provisions engine + build (#279) | add `"mutation"` to `gates` |
| e2e-verify | n/a (freshness only) | add `"e2e-verify"` to `gates` |
| **packaging** | **No** — requires caller `packaging_artifact` | **build the dist in a bespoke caller job + upload + wire `packaging_artifact`** |

## The ask

Teach the reusable `packaging` job to **auto-provision the distribution build** from the derived package root, mirroring coverage/mutation:

- python (`python_env=uv` + maturin backend): `uv build` / `maturin build` into a temp `dist/`.
- typescript (napi package declared): `pnpm build` + `pnpm pack`.
- rust (`Cargo.toml`): `cargo package`.

Then scan the just-built artifacts with the existing per-extension logic. `packaging_artifact` stays as the explicit escape hatch for repos that prefer to supply a prebuilt artifact, but a native monorepo can adopt with just `gates: ["packaging"]` and no caller build job.

## Acceptance

- A native-building monorepo adds `"packaging"` to a package's `gates` and the reusable job builds + scans that package's distribution with **no** caller-side build job and **no** `packaging_artifact`.
- dirsql deletes `packaging.yml` and adopts packaging exactly like coverage/mutation — one allowlist entry per package.

## Environment

Reusable workflow `@v0` (`174550e7`). The `packaging` job today: downloads `packaging_artifact` (or scans a checkout `dist/`) and runs `packaging <dist> --language <auto>` per file; it never builds. Consumer: dirsql `main`, `packages/{python,ts,rust}`.
