// Launcher entry: spawn the resolved `dirsql` binary and forward argv,
// exit code, and signals. When `argv[0] === "interpret"` the in-process
// TS helper handles the subcommand directly so a Rust orchestrator can
// spawn this script for native-language configs (#196) without depending
// on the bundled Rust binary.

import { spawnSync } from "node:child_process";
import { die } from "./die.js";
import { resolveBinary } from "./resolveBinary.js";

export function main(argv: string[] = process.argv.slice(2)): void {
  if (argv[0] === "interpret") {
    // Lazy import keeps SDK / `node:readline` out of the non-interpret hot path.
    import("./interpret.js")
      .then(({ interpret }) => interpret(argv[1] ?? ""))
      .then((code) => process.exit(code))
      .catch((e) => {
        process.stderr.write(
          `dirsql interpret: ${e instanceof Error ? e.message : String(e)}\n`,
        );
        process.exit(1);
      });
    return;
  }

  const binary = resolveBinary();
  const result = spawnSync(binary, argv, { stdio: "inherit" });
  if (result.error) {
    die(result.error.message, 1);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  }
  process.exit(result.status ?? 1);
}
