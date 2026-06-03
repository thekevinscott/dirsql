// Launcher entry: spawn the resolved `dirsql` binary and forward argv,
// exit code, and signals.

import { spawnSync } from "node:child_process";
import { die } from "./die.js";
import { resolveBinary } from "./resolveBinary.js";

export function main(argv: string[] = process.argv.slice(2)): never {
  const binary = resolveBinary();
  const result = spawnSync(binary, argv, { stdio: "inherit" });
  if (result.error) die(result.error.message, 1);
  if (result.signal) process.kill(process.pid, result.signal);
  process.exit(result.status ?? 1);
}
