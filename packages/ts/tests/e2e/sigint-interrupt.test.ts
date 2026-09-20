// CLI e2e: SIGINT ends a run that is still in progress.
//
// Spawns the real launcher over a real config whose `on-file` hook blocks, so
// the core is provably mid-scan when the signal lands, then asserts the
// process dies of it. Nothing is mocked: real launcher, real process, real
// filesystem, real core.

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..", "..");

const PKG: { bin: { dirsql: string } } = JSON.parse(
  await readFile(join(PKG_ROOT, "package.json"), "utf8"),
);
const CLI_ENTRY = join(PKG_ROOT, PKG.bin.dirsql);

async function waitUntilScanning(ready: string, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(ready)) {
      return;
    }
    await delay(50);
  }
  throw new Error("the scan never reached the on-file hook");
}

describe("SIGINT during a scan (CLI)", () => {
  let dir: string;
  let configPath: string;
  let ready: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-sigint-e2e-"));
    configPath = join(dir, ".dirsql.toml");
    ready = join(dir, "scanning");
    await mkdir(join(dir, "data"), { recursive: true });
    await writeFile(join(dir, "data", "a.txt"), "hello");
    await writeFile(
      configPath,
      `
[[table]]
name = "files"
ddl = "CREATE TABLE files (path TEXT)"
glob = "data/*.txt"
on-file = "sh -c 'touch ${ready}; sleep 120; printf \\"[]\\"'"
`,
    );
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("kills the process", async () => {
    const proc = spawn(
      process.execPath,
      [CLI_ENTRY, "query", "SELECT * FROM files", "--config", configPath],
      { cwd: dir, detached: true, stdio: ["ignore", "ignore", "ignore"] },
    );
    const died = new Promise<[number | null, NodeJS.Signals | null]>(
      (resolve) => proc.on("close", (code, signal) => resolve([code, signal])),
    );

    try {
      await waitUntilScanning(ready);
      proc.kill("SIGINT");
      const settled = await Promise.race([
        died,
        delay(15_000).then(() => "still running" as const),
      ]);
      expect(settled).toEqual([null, "SIGINT"]);
    } finally {
      const pid = proc.pid;
      if (pid && proc.exitCode === null && proc.signalCode === null) {
        // The blocking hook is a child in the same group; take the group.
        process.kill(-pid, "SIGKILL");
        await died;
      }
    }
  }, 60_000);
});
