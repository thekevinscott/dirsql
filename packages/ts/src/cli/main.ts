// Launcher entry: run the CLI in-process through the napi addon.
//
// The launcher is a transparent forwarder — every argv (including any
// subcommand) goes straight to the core's `runCli`, which owns subcommand
// dispatch and clap-rejects unknown ones. Nothing is spawned: the same
// `@dirsql/lib-*` addon the SDK loads carries the CLI, so a package ships one
// copy of the core instead of two (#739).

import { mainInProcess } from "bin-shim";
import { die } from "./die.js";
import { keepSignalsFatal } from "./keep-signals-fatal.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";
import { resolveRunCli } from "./resolve-run-cli.js";

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
