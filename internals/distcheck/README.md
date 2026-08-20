# `internals/distcheck`

Repo-only packaging distcheck flows for dirsql, invoked from `.github/workflows/`
(never published to an index). Extracted from the former
the former per-package packaging suites (#520, epic
#517): packaging distcheck is repo tooling that exercises the **release pipeline**,
not a behavior test of an SDK, so it lives here rather than in an SDK package's
`tests/`.

`dirsql-distcheck` is a single click group (`src/distcheck/cli.py`); each flow is a
`@click.command()` in its own subfolder, registered on the group as a
subcommand. Run a flow locally:

```bash
# build -> pack -> install -> run the published PyPI wheel (needs maturin on
# PATH)
uv run --project internals/distcheck dirsql-distcheck python

# build -> pack -> install -> run the published npm artifact set (needs
# `pnpm build` in packages/ts, and npm + pnpm on PATH)
uv run --project internals/distcheck dirsql-distcheck node
```

The node flow drives `npm` / `pnpm` via subprocess from Python -- it is repo
tooling, so one tested home matters more than language purity of the harness.

## What the flows verify

- **`python`** -- builds the wheel with `maturin build`, `pip install`s it into a
  fresh venv, and runs the installed `dirsql --version` console script plus
  `import dirsql`. Nothing is staged: since #738 the wheel's extension module
  carries the CLI and the console script calls it in-process.
- **`node`** -- packs the main `dirsql` package and a reconstructed host
  `@dirsql/lib-<slug>` sub-package, `npm install`s both into a fresh dir, and
  runs `node_modules/.bin/dirsql --version`, cross-checking the installed
  sub-package layout. Nothing separate is staged for the CLI: since #739 the
  napi addon carries it and the launcher calls it in-process.

**Caveat (host triple only):** each flow tests the host triple/interpreter.
Cross-target coverage lives in the release pipeline's install matrix (one runner
per target).

## Layout

Each flow follows the same shape (mirroring `internals/checks`):

- `<flow>/gate.py` -- the orchestration `run(...)`. Effects funnel through an
  injected `runner` (`subprocess.run`) and a `FileSystem` seam (`src/distcheck/filesystem.py`),
  plus pure helpers (command builders, wheel-tag / tarball selection, host
  detection), so the whole flow is unit-testable without a real build.
- `<flow>/cli.py` -- a thin `@click.command()` that resolves the checkout paths
  and calls `gate.run`, turning a `DistcheckError` into a non-zero exit.

Every module (except empty `__init__.py`s) carries a colocated `*_test.py`,
gated by `internals-distcheck-ci.yml`'s `internals-distcheck` job: `colocated-test`,
`unit-lint`, `integration-lint`, `unit-coverage`, and `mutation`.

## Test tiers

- colocated `*_test.py` units under `src/` -- isolate each unit with a mocked
  `runner` / `fs`.
- `tests/integration/` -- exercises each flow's `gate.run()` against **real**
  subprocesses and filesystem (the behavior the old per-package packaging suites had).
  Skips when the build prerequisites are absent, since the flow's actual CI
  execution is the `dirsql-distcheck <flow>` job, which builds them first.

No e2e tier / attestation: `tests/integration/` is the outermost tier, and the
CI `distcheck` jobs (`dirsql-python-ci.yml` / `dirsql-typescript-ci.yml`) run the real flows directly.

```bash
uv run --project internals/distcheck python -m pytest internals/distcheck/src internals/distcheck/tests -q
```

## Adding a flow

1. Create `src/distcheck/<new_flow>/` with `__init__.py`, `gate.py` (+ test), and
   `cli.py` (+ test).
2. Register it in `src/distcheck/cli.py`: import the command and
   `main.add_command(..., name="...")`.
3. Add it to `src/distcheck/cli_test.py`'s registration test.
4. Wire the workflow step that needs it to
   `uv run --project internals/distcheck dirsql-distcheck <flow>`.
