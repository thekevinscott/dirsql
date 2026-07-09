# `internals/checks`

Repo-only CI helper checks for dirsql, invoked from `.github/workflows/` (never published to an
index). `dirsql-checks` is a single click group (`src/checks/cli.py`); each check is a
`@click.command()` in its own subfolder, registered on the group as a subcommand.

Run any check locally:

```bash
uv run --project internals/checks dirsql-checks <check> [args...]
```

## Layout

Each check follows the same shape:

- `<check>/gate.py` -- the orchestration function, taking injected collaborators as keyword
  arguments (mirroring `pytest_gate`'s `runner=subprocess.run` pattern) so it's testable without
  mocking a module.
- `<check>/decide.py` / `<check>/git_ops.py` (as needed) -- pure decision logic and thin
  subprocess wrappers, split out when a check does more than a few lines of orchestration.
- `<check>/cli.py` -- a thin `@click.command()` that reads arguments/env vars and calls `gate.run`,
  raising `SystemExit` with its return code.

Every module (except empty `__init__.py`s) carries a colocated `*_test.py`, gated by
`conventions.yml`'s `internals-checks` job: `colocated-test`, `unit-lint`, `unit-coverage`,
`mutation`, and `e2e-verify`.

## Test tiers

- `tests/integration/` -- exercises each check's `gate.run()` against real collaborators (real
  `git`, real pytest subprocess) rather than the packaged CLI. Gated by the `integration-lint` gate
  in `conventions.yml`'s `internals-checks` job, which derives `tests/integration/` from the package
  root (#417).
- `tests/e2e/` -- spawns the real `dirsql-checks` CLI as a subprocess with nothing mocked. Not run
  in CI; gated only via `e2e-attestation.json` freshness (see AGENTS.md, "E2E Attestation").

Run locally:

```bash
uv run --project internals/checks python -m pytest internals/checks/tests/integration -q
uv run --project internals/checks python -m pytest internals/checks/tests/e2e -q
```

Refresh the attestation after changing `internals/checks`:

```bash
cd internals/checks && uvx testing-conventions e2e attest 'uv run python -m pytest tests/e2e -q'
```

## Adding a check

1. Create `src/checks/<new_check>/` with `__init__.py`, `gate.py` (+ test), and `cli.py` (+ test).
2. Register it in `src/checks/cli.py`: import the command and `main.add_command(..., name="...")`.
3. Add it to `src/checks/cli_test.py`'s registration test.
4. Wire the workflow step that needs it to `uv run --project internals/checks dirsql-checks <check>`.
