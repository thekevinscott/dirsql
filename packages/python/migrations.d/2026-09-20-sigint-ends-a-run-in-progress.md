### SIGINT ends a run in progress through the Python launcher

**Summary**

The `dirsql` console script installed an empty SIGINT handler for the duration of the core's run. A signal arriving during a long directory scan was therefore discarded, and the process could only be ended with SIGTERM. SIGINT is now left at its default disposition for that window, so the kernel terminates the process immediately. No API changes; the observable difference is that a signal that used to do nothing now ends the process with the usual 128+SIGINT status.

**Required changes**

| Before | After |
| --- | --- |
| `kill -INT <pid>` during a query: ignored; the run continues to completion | `kill -INT <pid>` during a query: the process dies of SIGINT (shell reports `130`) |
| Ctrl-C while the REPL is executing a statement: ignored | Ctrl-C while the REPL is executing a statement: the session ends |

A script that sent SIGINT to a `dirsql` process and relied on it being ignored must stop sending it. A script that escalated to SIGTERM after an ignored SIGINT can drop the escalation.

`dirsql server` is unchanged: its graceful shutdown still exits `0` on SIGINT and SIGTERM. Ctrl-C at an idle REPL prompt is unchanged too — reedline reads it as a key event, not a signal.

**Deprecations removed**

_None._

**Behavior changes without code changes**

The exit status of an interrupted run changes from "no exit" to death by SIGINT. This aligns the Python launcher with the standalone `dirsql` binary, which has always behaved this way.

**Verification**

Against a directory large enough to take a few seconds to scan:

```bash
dirsql "SELECT count(*) FROM './'"   # press Ctrl-C while it runs
echo $?
```

Expected: the process ends promptly and the shell reports `130`. Before this change it kept running and the SIGINT was discarded. Run it in the foreground: a non-interactive shell sets SIGINT to `SIG_IGN` for `&` background jobs, which hides the difference.
