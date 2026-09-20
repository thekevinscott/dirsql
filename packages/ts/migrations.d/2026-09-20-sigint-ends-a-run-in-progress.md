### SIGINT ends a run in progress through the npm launcher

**Summary**

The npm launcher registered JS listeners for SIGINT and SIGTERM, but the napi `runCli` export is synchronous and blocks the event loop for the whole of the core's run, so neither listener could run while a scan was in progress. A signal arriving then was absorbed and the process could only be ended with SIGKILL. The addon now sets both signals to their default disposition for the duration of the core's run and restores the listeners' disposition after. No API changes; the observable difference is that a signal that used to do nothing now ends the process with the usual 128+signal status.

**Required changes**

| Before | After |
| --- | --- |
| `kill -INT <pid>` during a query: absorbed; the run continues to completion | `kill -INT <pid>` during a query: the process dies of SIGINT (shell reports `130`) |
| `kill -TERM <pid>` during a query: absorbed; SIGKILL was the only way out | `kill -TERM <pid>` during a query: the process dies of SIGTERM (shell reports `143`) |

A script that sent SIGINT to a `dirsql` process and relied on it being ignored must stop sending it. A script that escalated to SIGKILL after an absorbed signal can drop the escalation.

`dirsql server` is unchanged: its graceful shutdown still exits `0` on SIGINT and SIGTERM.

**Deprecations removed**

_None._

**Behavior changes without code changes**

The exit status of an interrupted run changes from "no exit" to death by the signal. This aligns the npm launcher with the standalone `dirsql` binary and with the Python launcher.

**Verification**

Against a directory large enough to take a few seconds to scan:

```bash
npx dirsql "SELECT count(*) FROM './'"   # press Ctrl-C while it runs
echo $?
```

Expected: the process ends promptly and the shell reports `130`. Before this change it kept running and the signal was absorbed. Run it in the foreground: a non-interactive shell sets SIGINT to `SIG_IGN` for `&` background jobs, which hides the difference.
