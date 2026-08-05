# npm CLI runs in-process; the `@dirsql/cli-*` family is retired

## Summary

The `dirsql` CLI no longer spawns a bundled binary. The napi addon
(`@dirsql/lib-<platform>`) is built with the core's `cli` feature and exports
`runCli`; the launcher calls it in this process via bin-shim. The
`@dirsql/cli-<platform>` family, which shipped a second copy of the same core
as a standalone executable, is no longer published — cutting the per-platform
native payload from 10,139,000 B to 5,799,904 B (−42.8%).

## Required changes

**For nearly everyone: none.** `npx dirsql`, `npm i -g dirsql`, and the
`dirsql` bin behave exactly as before — same argv handling, same output, same
exit codes.

Action is needed only if you depend on **`@dirsql/cli-<platform>` directly**
rather than through the `dirsql` package — for example pinning it in your own
`optionalDependencies`, or locating its binary to exec yourself:

- Depend on `@dirsql/lib-<platform>` instead and call its `runCli(argv)`
  export, which returns an exit code rather than terminating the process.
- If you genuinely need a standalone executable, install the Rust binary
  with `cargo install dirsql --features cli`. It is the same code.

## Deprecations removed

`@dirsql/cli-<platform>` (all five platforms) is no longer published. Existing
published versions remain on npm; no new ones are cut.

## Behavior changes without code changes

- **The CLI shares the launcher's process.** For `dirsql server` this means
  the Node process stays resident for the server's lifetime instead of being
  replaced by a child. Ctrl-C still shuts down gracefully and exits 0.
- **One fewer process per invocation.** `spawnSync` of a ~5.6 MB binary is
  replaced by a `dlopen` of an addon that is loaded anyway.
- **Signals are handled by the launcher's listeners.** The launcher installs
  `SIGINT`/`SIGTERM` listeners before handing control to the core. This is
  load-bearing, not defensive: signal-hook (which tokio uses) chains to the
  handler installed before it, and bare Node leaves `SIG_DFL`, which it does
  not emulate — without a prior listener a signalled process is swallowed and
  becomes SIGKILL-only. Verified both ways (see Verification).

## Verification

Built the addon, then exercised the installed launcher end to end:

```
$ dirsql --version                              # 0    dirsql 0.2.7
$ dirsql query "SELECT COUNT(*) AS n FROM './'" # 0    [{"n":2}]
$ dirsql query "SELECT frobnicate()"            # 1    SQLite error: no such function
$ dirsql --nope                                 # 2    error: unexpected argument
$ dirsql server --port 7231 & kill -INT $!      # 0    graceful shutdown in ~200ms
```

The signal listener was verified to be load-bearing with a negative control:
the identical addon and `runCli` call, with no listener installed, ignored
SIGINT and had to be SIGKILLed; with the listener it exits cleanly in ~200 ms.
