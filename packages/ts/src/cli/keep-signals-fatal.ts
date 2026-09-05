/**
 * Keep Ctrl-C fatal while the CLI runs in-process.
 *
 * The core installs tokio signal handlers for `dirsql server`, and
 * signal-hook (which tokio uses) *chains*: it runs its own actions and then
 * the handler installed before it. CPython installs `default_int_handler` at
 * startup, so the pip launcher gets a terminating disposition for free —
 * bare Node leaves `SIG_DFL`, which signal-hook does not emulate, so the
 * signal is swallowed and the process becomes SIGKILL-only (measured: a
 * probe ignored SIGTERM for 143 seconds).
 *
 * Registering a JS listener first gives signal-hook a real prior handler to
 * chain to. This cannot be fixed after the fact: `removeAllListeners` only
 * drops JS listeners and cannot touch a disposition installed by native
 * code, and `unregister_signal` leaves the OS handler in place. One listener
 * is the whole remedy — no `unsafe`, no `sigaction`.
 */
export function keepSignalsFatal(): void {
  // While the core is running, its own handler drives shutdown and this
  // listener is the tail of the chain; it matters for signals arriving
  // outside that window, where exiting is exactly right.
  process.on("SIGINT", () => process.exit(130));
  process.on("SIGTERM", () => process.exit(143));
}
