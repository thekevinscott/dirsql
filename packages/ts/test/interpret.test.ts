// Integration tests for `dirsql interpret` -- the long-running native
// config helper (#196).
//
// Spawns the real `dist/bin/dirsql.js` launcher as a subprocess and talks
// NDJSON over stdin/stdout. No monkeypatching, no in-process shortcut.
//
// NDJSON protocol (per #196):
//   handshake (helper -> caller, once on startup):
//     {"type": "config", "state": <app.toJSON()>}
//   extract request (caller -> helper):
//     {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}
//   extract response (helper -> caller):
//     {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
//     {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}

import { type ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createInterface, type Interface as ReadlineInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..");

// Resolve the CLI entry from package.json's `bin` field so a future rename
// of bin's location is picked up automatically.
const PKG: { bin: { dirsql: string } } = JSON.parse(
  readFileSync(join(PKG_ROOT, "package.json"), "utf8"),
);
const CLI_ENTRY = join(PKG_ROOT, PKG.bin.dirsql);

const FIXTURE_DIR = join(__dirname, "__fixtures__", "interpret");
const HAPPY_CONFIG = join(FIXTURE_DIR, "dirsql.config.mjs");
const RAISES_CONFIG = join(FIXTURE_DIR, "dirsql.config_raises.mjs");
const NO_DEFAULT_CONFIG = join(FIXTURE_DIR, "dirsql.config_no_default.mjs");
const ALPHA_PATH = join(FIXTURE_DIR, "data", "a", "meta.json");

interface ResultMessage {
  type: "result";
  id: number;
  ok: boolean;
  rows?: unknown[];
  error?: string;
}

interface ConfigMessage {
  type: "config";
  state: {
    root: string;
    tables: { ddl: string; glob: string; strict: boolean }[];
    ignore: string[];
    persist: boolean;
    persistPath: string | null;
  };
}

function spawnInterpret(
  configPath: string,
): { proc: ChildProcessWithoutNullStreams; lines: AsyncIterator<string>; rl: ReadlineInterface; stderrChunks: string[] } {
  if (!existsSync(CLI_ENTRY)) {
    throw new Error(
      `CLI entry not built: ${CLI_ENTRY} -- run \`pnpm build\` first`,
    );
  }
  const proc = spawn(process.execPath, [CLI_ENTRY, "interpret", configPath], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  const rl = createInterface({ input: proc.stdout });
  const stderrChunks: string[] = [];
  proc.stderr.setEncoding("utf8");
  proc.stderr.on("data", (chunk: string) => stderrChunks.push(chunk));
  return { proc, lines: rl[Symbol.asyncIterator](), rl, stderrChunks };
}

async function readLine(
  lines: AsyncIterator<string>,
  stderrChunks: string[],
  timeoutMs = 5_000,
): Promise<string> {
  const next = lines.next();
  const timer = new Promise<{ done: true; value: undefined }>((_resolve, reject) => {
    setTimeout(
      () => reject(new Error(`timed out reading line; stderr: ${stderrChunks.join("")}`)),
      timeoutMs,
    );
  });
  const result = (await Promise.race([next, timer])) as IteratorResult<string>;
  if (result.done) {
    throw new Error(
      `helper exited before writing a line; stderr: ${stderrChunks.join("")}`,
    );
  }
  return result.value;
}

function send(proc: ChildProcessWithoutNullStreams, msg: unknown): void {
  proc.stdin.write(`${JSON.stringify(msg)}\n`);
}

async function shutdown(
  proc: ChildProcessWithoutNullStreams,
  rl: ReadlineInterface,
): Promise<void> {
  rl.close();
  proc.stdin.end();
  await new Promise<void>((resolve) => {
    if (proc.exitCode !== null || proc.signalCode !== null) {
      resolve();
    } else {
      proc.once("close", () => resolve());
      setTimeout(() => {
        if (proc.exitCode === null && proc.signalCode === null) {
          proc.kill("SIGKILL");
        }
      }, 5_000);
    }
  });
}

describe("dirsql interpret (#196)", () => {
  let handle:
    | ReturnType<typeof spawnInterpret>
    | undefined;

  beforeEach(() => {
    handle = undefined;
  });

  afterEach(async () => {
    if (handle) await shutdown(handle.proc, handle.rl);
  });

  it("handshake `state` matches `app.toJSON()`", async () => {
    handle = spawnInterpret(HAPPY_CONFIG);
    const msg = JSON.parse(
      await readLine(handle.lines, handle.stderrChunks),
    ) as ConfigMessage;
    expect(msg.type).toBe("config");
    // Same keys as `app.toJSON()` per test/serialization.test.ts.
    expect(Object.keys(msg.state).sort()).toEqual(
      ["ignore", "persist", "persistPath", "root", "tables"].sort(),
    );
    expect(msg.state.root).toBe(join(FIXTURE_DIR, "data"));
    expect(msg.state.tables).toHaveLength(1);
    expect(msg.state.tables[0].ddl).toBe("CREATE TABLE papers (title TEXT)");
    expect(msg.state.tables[0].glob).toBe("**/meta.json");
    expect(msg.state.tables[0].strict).toBe(false);
    expect(msg.state.ignore).toEqual([]);
    expect(msg.state.persist).toBe(false);
    expect(msg.state.persistPath).toBeNull();
  });

  it("single extract request returns ok=true with fixture rows", async () => {
    handle = spawnInterpret(HAPPY_CONFIG);
    await readLine(handle.lines, handle.stderrChunks); // handshake
    send(handle.proc, {
      type: "extract",
      id: 1,
      table: "papers",
      path: ALPHA_PATH,
    });
    const response = JSON.parse(
      await readLine(handle.lines, handle.stderrChunks),
    ) as ResultMessage;
    expect(response.type).toBe("result");
    expect(response.id).toBe(1);
    expect(response.ok).toBe(true);
    expect(response.rows).toEqual([{ title: "Alpha" }]);
  });

  it("extract callback exception surfaces as ok=false with error string", async () => {
    handle = spawnInterpret(RAISES_CONFIG);
    await readLine(handle.lines, handle.stderrChunks); // handshake
    send(handle.proc, {
      type: "extract",
      id: 7,
      table: "papers",
      path: ALPHA_PATH,
    });
    const response = JSON.parse(
      await readLine(handle.lines, handle.stderrChunks),
    ) as ResultMessage;
    expect(response.type).toBe("result");
    expect(response.id).toBe(7);
    expect(response.ok).toBe(false);
    expect(response.error).toContain("synthetic extract failure");
  });

  it("unknown table name returns ok=false", async () => {
    handle = spawnInterpret(HAPPY_CONFIG);
    await readLine(handle.lines, handle.stderrChunks); // handshake
    send(handle.proc, {
      type: "extract",
      id: 3,
      table: "nonexistent",
      path: ALPHA_PATH,
    });
    const response = JSON.parse(
      await readLine(handle.lines, handle.stderrChunks),
    ) as ResultMessage;
    expect(response.type).toBe("result");
    expect(response.id).toBe(3);
    expect(response.ok).toBe(false);
    expect(response.error).toContain("nonexistent");
  });

  it("config without a default export exits non-zero with clean stderr", async () => {
    // Direct spawn / communicate — no handshake expected here.
    const proc = spawn(
      process.execPath,
      [CLI_ENTRY, "interpret", NO_DEFAULT_CONFIG],
      { stdio: ["pipe", "pipe", "pipe"] },
    );
    let stderr = "";
    proc.stderr.setEncoding("utf8");
    proc.stderr.on("data", (chunk: string) => {
      stderr += chunk;
    });
    proc.stdin.end();
    const exitCode = await new Promise<number | null>((resolve) => {
      proc.on("close", (code) => resolve(code));
      setTimeout(() => {
        if (proc.exitCode === null) proc.kill("SIGKILL");
      }, 10_000);
    });
    expect(exitCode).not.toBe(0);
    // "Clean": no V8 stack trace.
    expect(stderr).not.toMatch(/\s+at [^\s]+ \(/);
    expect(stderr.toLowerCase()).toMatch(/default|export/);
  });
});
