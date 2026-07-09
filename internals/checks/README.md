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
`conventions.yml`'s `internals-checks` job: `colocated-test`, `unit-lint`, `unit-coverage`, and
`mutation`.

## Adding a check

1. Create `src/checks/<new_check>/` with `__init__.py`, `gate.py` (+ test), and `cli.py` (+ test).
2. Register it in `src/checks/cli.py`: import the command and `main.add_command(..., name="...")`.
3. Add it to `src/checks/cli_test.py`'s registration test.
4. Wire the workflow step that needs it to `uv run --project internals/checks dirsql-checks <check>`.
