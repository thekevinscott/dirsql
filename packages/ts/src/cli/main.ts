// Launcher entry: run the CLI in-process through the napi addon.
//
// The launcher is a transparent forwarder — every argv (including any
// subcommand) goes straight to the core's `runCli`, which owns subcommand
// dispatch and clap-rejects unknown ones. Nothing is spawned: the same
// `@dirsql/lib-*` addon the SDK loads carries the CLI, so a package ships one
// copy of the core instead of two (#739).

import { createRequire } from "node:module";
import { mainInProcess } from "bin-shim";
import { loadNativeCore } from "../load-native-core.js";
import { die } from "./die.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

/** The addon's CLI entry point, as napi exports it. */
type RunCli = (argv: readonly string[], version?: string) => number;

/**
 * This package's version, or `undefined` if its manifest cannot be read.
 *
 * `package.json` sits two levels above this module both in `src/` and in
 * `dist/`. Falling back rather than throwing keeps a `--version` the addon can
 * still answer (with its own, wrong number) preferable to a CLI that refuses to
 * start.
 */
export function packageVersion(
  requirer: (specifier: string) => unknown = createRequire(import.meta.url),
): string | undefined {
  try {
    const { version } = requirer("../../package.json") as { version?: unknown };
    return typeof version === "string" ? version : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Resolve `runCli` off the same addon the SDK loads.
 *
 * bin-shim can resolve an addon itself, but its default naming is
 * `@{scope}/lib-{platform}-{arch}`, which cannot express the ABI suffix
 * dirsql's packages carry (`@dirsql/lib-linux-x64-gnu`). Reusing
 * `loadNativeCore` keeps one resolution rule for the SDK and the CLI —
 * including its dev fallback to a locally built `dirsql.node` — so a
 * monorepo checkout and a published install take the same path.
 *
 * The addon is handed this package's version, because the core crate compiled
 * into it carries a literal no npm release rewrites (#958).
 */
function resolveRunCli(): (argv: readonly string[]) => number {
  const core = loadNativeCore() as { runCli?: unknown };
  if (typeof core.runCli !== "function") {
    throw new Error(
      "dirsql: the native addon has no callable `runCli` export; " +
        "it was built without the `cli` feature.",
    );
  }
  const runCli = core.runCli as RunCli;
  const version = packageVersion();
  return (argv) => runCli(argv, version);
}

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
function keepSignalsFatal(): void {
  // While the core is running, its own handler drives shutdown and this
  // listener is the tail of the chain; it matters for signals arriving
  // outside that window, where exiting is exactly right.
  process.on("SIGINT", () => process.exit(130));
  process.on("SIGTERM", () => process.exit(143));
}

export async function main(
  argv: string[] = process.argv.slice(2),
): Promise<void> {
  keepSignalsFatal();
  // Resolve any package-name extensions in a TOML config here (the core
  // can't) and pass them as `--extension` flags; a no-op otherwise.
  const resolved = await withResolvedExtensions(argv);
  let code: number;
  try {
    code = await mainInProcess({
      argv: resolved,
      binaryName: "dirsql",
      // Resolution is ours (see `resolveRunCli`), so bin-shim never consults
      // its own naming; `from` only satisfies the shared options type.
      from: import.meta.url,
      runCli: resolveRunCli(),
    });
  } catch (e: unknown) {
    die(e instanceof Error ? e.message : String(e), 1);
    return;
  }
  process.exit(code);
}
