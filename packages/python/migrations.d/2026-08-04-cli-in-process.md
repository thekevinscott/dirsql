# The wheel's CLI runs in-process; `dirsql/_binary/` is gone

## Summary

The `dirsql` console script no longer `exec`s a bundled binary. The extension
module (`dirsql._dirsql`) is built with the core's `cli` feature and exports
`run_cli`; the launcher calls it in this process via bin-shim. The wheel
therefore ships one copy of the core — the extension module — instead of that
module *plus* a standalone executable.

## Required changes

**For nearly everyone: none.** `pip install dirsql`, `uvx dirsql`, and the
`dirsql` console script behave as before: same argv handling, same output,
same exit codes.

Action is needed only if you located the bundled binary by path — e.g.
`importlib.resources.files("dirsql") / "_binary" / "dirsql"` — and executed it
yourself:

- Call `dirsql._dirsql.run_cli(argv)` instead. It returns an exit code rather
  than terminating the process (`argv` excludes the program name).
- If you need a standalone executable, `cargo install dirsql --features cli`
  builds the same code.

## Deprecations removed

`dirsql/_binary/` is no longer part of the wheel. The
`{ path = "dirsql/_binary/*", format = "wheel" }` maturin include row and the
pypi `[package.bundle_cli]` release block are both removed.

`dirsql.cli.binary_path` and `dirsql.cli.is_windows` are deleted — both existed
only to locate and exec that binary.

## Behavior changes without code changes

- **The CLI shares the console script's process.** For `dirsql server` the
  Python interpreter stays resident for the server's lifetime rather than
  being replaced by `os.execv`.
- **Windows and POSIX take the same path.** The `is_windows()` branch
  (`subprocess.run` vs `os.execv`) is gone, so the platforms cannot drift.
- **`dirsql server` + Ctrl-C still exits 0.** This needed care and is worth
  recording. signal-hook (which tokio uses) chains to the handler installed
  before it; CPython's `default_int_handler` raises `KeyboardInterrupt`, which
  lands *after* `run_cli` has already returned 0 from its graceful shutdown
  and which bin-shim then converts to 130. Measured: the naive wiring exits
  **130** where the old `execv` path exits **0**. The launcher therefore
  installs a non-raising SIGINT handler for the duration of the run so the
  core's own exit code survives, and restores the previous handler afterward.
  This keeps the wheel's behavior identical to before *and* matches the npm
  launcher (#739), which also exits 0.

## Verification

```
$ dirsql --version                              # 0   dirsql 0.2.7
$ dirsql query "SELECT COUNT(*) AS n FROM './'" # 0   [{"n":2}]
$ dirsql query "SELECT frobnicate()"            # 1   SQLite error: no such function
$ dirsql --nope                                 # 2   error: unexpected argument
$ dirsql server --port 7242 & kill -INT $!      # 0   graceful shutdown
```

Python e2e suite: 27 passed. Unit suite: 108 passed at 100% coverage.
