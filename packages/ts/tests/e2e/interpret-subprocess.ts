// Subprocess plumbing for the `dirsql interpret` integration tests.
//
// Kept in its own module (not a `.test.ts` file) so vitest doesn't try
// to collect it. Imports are runtime-only; nothing here is part of the
// shipped TS SDK.

import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { access } from "node:fs/promises";
import {
  type Interface as ReadlineInterface,
  createInterface,
} from "node:readline";

export interface InterpretHandle {
  proc: ChildProcessWithoutNullStreams;
  lines: AsyncIterator<string>;
  rl: ReadlineInterface;
  stderrChunks: string[];
}

/** Start `<node> <cliEntry> interpret <configPath>` with piped streams. */
export async function spawnInterpret(
  cliEntry: string,
  configPath: string,
  opts: { cwd?: string } = {},
): Promise<InterpretHandle> {
  try {
    await access(cliEntry);
  } catch {
    throw new Error(
      `CLI entry not built: ${cliEntry} -- run \`pnpm build\` first`,
    );
  }
  const proc = spawn(process.execPath, [cliEntry, "interpret", configPath], {
    stdio: ["pipe", "pipe", "pipe"],
    cwd: opts.cwd,
  });
  const rl = createInterface({ input: proc.stdout });
  const stderrChunks: string[] = [];
  proc.stderr.setEncoding("utf8");
  proc.stderr.on("data", (chunk: string) => stderrChunks.push(chunk));
  return { proc, lines: rl[Symbol.asyncIterator](), rl, stderrChunks };
}

/** Read one stdout line; reject with a stderr-bearing error on timeout/EOF. */
export async function readLine(
  handle: InterpretHandle,
  timeoutMs = 5_000,
): Promise<string> {
  const next = handle.lines.next();
  const timer = new Promise<{ done: true; value: undefined }>(
    (_resolve, reject) => {
      setTimeout(
        () =>
          reject(
            new Error(
              `timed out reading line; stderr: ${handle.stderrChunks.join("")}`,
            ),
          ),
        timeoutMs,
      );
    },
  );
  const result = (await Promise.race([next, timer])) as IteratorResult<string>;
  if (result.done) {
    throw new Error(
      `helper exited before writing a line; stderr: ${handle.stderrChunks.join("")}`,
    );
  }
  return result.value;
}

/** Write one NDJSON line to the helper's stdin. */
export function send(handle: InterpretHandle, msg: unknown): void {
  handle.proc.stdin.write(`${JSON.stringify(msg)}\n`);
}

/** Close stdin and wait for the helper to exit; SIGKILL after 5s. */
export async function shutdown(handle: InterpretHandle): Promise<void> {
  handle.rl.close();
  handle.proc.stdin.end();
  await new Promise<void>((resolve) => {
    if (handle.proc.exitCode !== null || handle.proc.signalCode !== null) {
      resolve();
    } else {
      handle.proc.once("close", () => resolve());
      setTimeout(() => {
        if (handle.proc.exitCode === null && handle.proc.signalCode === null) {
          handle.proc.kill("SIGKILL");
        }
      }, 5_000);
    }
  });
}
