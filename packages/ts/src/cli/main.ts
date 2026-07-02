// Launcher entry: resolve the bundled `dirsql` binary and forward argv,
// exit code, and signals to it. The launcher is a transparent forwarder —
// every argv (including any subcommand) goes straight to the Rust binary,
// which owns subcommand dispatch and clap-rejects unknown ones.

import { spawnSync } from "node:child_process";
import { die } from "./die.js";
import { resolveBinary } from "./resolve-binary.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

export async function main(
  argv: string[] = process.argv.slice(2),
): Promise<void> {
  const binary = resolveBinary();
  // Resolve any package-name extensions in a TOML config here (the binary
  // can't) and pass them as `--extension` flags; a no-op otherwise (#227).
  const result = spawnSync(binary, withResolvedExtensions(argv), {
    stdio: "inherit",
  });
  if (result.error) {
    die(result.error.message, 1);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  }
  process.exit(result.status ?? 1);
}
