// Launcher entry: spawn the resolved `dirsql` binary and forward argv,
// exit code, and signals.

import { type SpawnSyncReturns, spawnSync } from "node:child_process";
import { die } from "./die.js";
import { resolveBinary } from "./resolveBinary.js";

export interface MainDeps {
  resolve: () => string;
  spawn: (binary: string, argv: string[]) => SpawnSyncReturns<Buffer>;
  dieFn: typeof die;
}

/* v8 ignore start - production bridges to external modules; covered e2e */
export const defaultDeps: MainDeps = {
  resolve: () => resolveBinary(),
  spawn: (binary, argv) => spawnSync(binary, argv, { stdio: "inherit" }),
  dieFn: die,
};
/* v8 ignore stop */

export function main(argv: string[], deps: MainDeps): never {
  const binary = deps.resolve();
  const result = deps.spawn(binary, argv);
  if (result.error) {
    deps.dieFn(result.error.message, 1);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  }
  process.exit(result.status ?? 1);
}
