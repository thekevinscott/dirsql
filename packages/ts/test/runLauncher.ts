// Spawn the TypeScript launcher under `tsx` with `NODE_PATH` pointed at
// a fake install root. Encapsulates the child-process plumbing so tests
// only assert on the result.

import { type SpawnSyncReturns, spawnSync } from "node:child_process";
import { join } from "node:path";

const LAUNCHER = join(import.meta.dirname, "..", "ts", "bin", "dirsql.ts");

export function runLauncher(
  fakeInstallRoot: string,
  argv: string[],
): SpawnSyncReturns<string> {
  return spawnSync(process.execPath, ["--import", "tsx", LAUNCHER, ...argv], {
    encoding: "utf8",
    env: {
      ...process.env,
      NODE_PATH: join(fakeInstallRoot, "node_modules"),
    },
  });
}
