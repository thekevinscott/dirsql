// E2E regression for the removal of the `interpret` subcommand and the
// native-language (.js/.mjs/.cjs) config path from the TypeScript SDK
// (epic #321, sub-issue A2 / #324).
//
// The TS launcher used to intercept `argv[0] === "interpret"` and run an
// in-process helper. That branch is gone: the launcher now forwards every
// argv verbatim to the bundled Rust binary, which owns subcommand dispatch
// and clap-rejects any unknown subcommand. This test spawns the REAL built
// launcher (`dist/cli/dirsql.js`) — nothing mocked, real process, real
// filesystem, real Rust binary — and asserts that `dirsql interpret <X>` is
// no longer dispatched: it exits non-zero and stderr is a clean clap error,
// not a JS stack trace.

import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..", "..");

// Resolve the CLI entry from package.json's `bin` field so a future rename
// of bin's location is picked up automatically.
const PKG: { bin: { dirsql: string } } = JSON.parse(
  await readFile(join(PKG_ROOT, "package.json"), "utf8"),
);
const CLI_ENTRY = join(PKG_ROOT, PKG.bin.dirsql);

interface RunResult {
  code: number | null;
  stderr: string;
  stdout: string;
}

function runLauncher(args: string[]): Promise<RunResult> {
  return new Promise((resolve) => {
    const proc = spawn(process.execPath, [CLI_ENTRY, ...args], {
      stdio: ["pipe", "pipe", "pipe"],
    });
    let stderr = "";
    let stdout = "";
    proc.stderr.setEncoding("utf8");
    proc.stdout.setEncoding("utf8");
    proc.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    proc.stdout.on("data", (chunk: string) => {
      stdout += chunk;
    });
    proc.stdin.end();
    proc.on("close", (code) => resolve({ code, stderr, stdout }));
    setTimeout(() => {
      if (proc.exitCode === null) {
        proc.kill("SIGKILL");
      }
    }, 10_000);
  });
}

describe("interpret subcommand removed (#324)", () => {
  it("no longer dispatches `interpret`; forwards to the Rust binary which rejects it", async () => {
    const { code, stderr } = await runLauncher([
      "interpret",
      "./dirsql.config.mjs",
    ]);

    // Non-zero: the binary clap-rejects an unknown subcommand.
    expect(code).not.toBe(0);
    // "Clean": a clap usage error, not a Node/V8 stack trace from a crashed
    // in-process interpret helper.
    expect(stderr).not.toMatch(/\s+at [^\s]+ \(/);
    expect(stderr).not.toMatch(/node:internal/);
  }, 15_000);
});
