// Launcher entry: spawn the resolved `dirsql` binary and forward argv,
// exit code, and signals. When `argv[0] === "interpret"` the
// in-process TS helper handles the subcommand directly so a Rust
// orchestrator can spawn this script for native-language configs
// (#196) without depending on the bundled Rust binary.

import { spawnSync } from "node:child_process";
import { die } from "./die.js";
import { interpret } from "./interpret/index.js";
import { resolveBinary } from "./resolve-binary.js";

export async function main(
  argv: string[] = process.argv.slice(2),
): Promise<void> {
  if (argv[0] === "interpret") {
    const code = await interpret(argv[1] ?? "");
    process.exit(code);
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
