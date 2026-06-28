// Integration tests for `dirsql interpret` -- the long-running native
// config helper (#196).
//
// Spawns the real `dist/cli/dirsql.js` launcher as a subprocess and talks
// NDJSON over stdin/stdout. No monkeypatching, no in-process shortcut.
// Subprocess plumbing lives in `./interpretSubprocess.ts`.
//
// NDJSON protocol (per #196):
//   handshake (helper -> caller, once on startup):
//     {"type": "config", "state": <app.toJSON()>}
//   extract request (caller -> helper):
//     {"type": "extract", "id": <int>, "table": "<name>", "path": "<abs>"}
//   extract response (helper -> caller):
//     {"type": "result", "id": <int>, "ok": true,  "rows": [...]}
//     {"type": "result", "id": <int>, "ok": false, "error": "<msg>"}

import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  type InterpretHandle,
  readLine,
  send,
  shutdown,
  spawnInterpret,
} from "./interpretSubprocess.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = join(__dirname, "..");

// Resolve the CLI entry from package.json's `bin` field so a future rename
// of bin's location is picked up automatically. Top-level await reads the
// manifest once at module load; the value is a module constant the tests read.
const PKG: { bin: { dirsql: string } } = JSON.parse(
  await readFile(join(PKG_ROOT, "package.json"), "utf8"),
);
const CLI_ENTRY = join(PKG_ROOT, PKG.bin.dirsql);

const FIXTURE_DIR = join(__dirname, "__fixtures__", "interpret");
const RAISES_CONFIG = join(FIXTURE_DIR, "dirsql.config_raises.mjs");
const NO_DEFAULT_CONFIG = join(FIXTURE_DIR, "dirsql.config_no_default.mjs");
const ALPHA_PATH = join(FIXTURE_DIR, "data", "a", "meta.json");

/** Happy-path config exists in three loader flavors; same shape. */
const HAPPY_EXTS = ["mjs", "js", "cjs"] as const;
const happyConfig = (ext: (typeof HAPPY_EXTS)[number]): string =>
  join(FIXTURE_DIR, `dirsql.config.${ext}`);

describe("dirsql interpret (#196)", () => {
  let handle: InterpretHandle | undefined;

  beforeEach(() => {
    handle = undefined;
  });

  afterEach(async () => {
    if (handle) {
      await shutdown(handle);
    }
  });

  describe("handshake", () => {
    it.each(HAPPY_EXTS)(
      "emits a config message whose state equals app.toJSON() (.%s loader)",
      async (ext) => {
        handle = await spawnInterpret(CLI_ENTRY, happyConfig(ext));
        expect(JSON.parse(await readLine(handle))).toEqual({
          type: "config",
          state: {
            root: join(FIXTURE_DIR, "data"),
            tables: [
              {
                ddl: "CREATE TABLE papers (title TEXT)",
                glob: "**/meta.json",
                strict: false,
              },
            ],
            ignore: [],
            persist: false,
            persistPath: null,
          },
        });
      },
    );
  });

  describe("extract", () => {
    it.each(HAPPY_EXTS)(
      "returns ok=true with the rows extract produced (.%s loader)",
      async (ext) => {
        handle = await spawnInterpret(CLI_ENTRY, happyConfig(ext));
        await readLine(handle); // drain handshake
        send(handle, {
          type: "extract",
          id: 1,
          table: "papers",
          path: ALPHA_PATH,
        });
        expect(JSON.parse(await readLine(handle))).toEqual({
          type: "result",
          id: 1,
          ok: true,
          rows: [{ title: "Alpha" }],
        });
      },
    );

    it("returns ok=false when the user extract throws", async () => {
      handle = await spawnInterpret(CLI_ENTRY, RAISES_CONFIG);
      await readLine(handle); // drain handshake
      send(handle, {
        type: "extract",
        id: 7,
        table: "papers",
        path: ALPHA_PATH,
      });
      expect(JSON.parse(await readLine(handle))).toEqual({
        type: "result",
        id: 7,
        ok: false,
        error: expect.stringContaining("synthetic extract failure"),
      });
    });

    it("returns ok=false when the request names an unknown table", async () => {
      handle = await spawnInterpret(CLI_ENTRY, happyConfig("mjs"));
      await readLine(handle); // drain handshake
      send(handle, {
        type: "extract",
        id: 3,
        table: "nonexistent",
        path: ALPHA_PATH,
      });
      expect(JSON.parse(await readLine(handle))).toEqual({
        type: "result",
        id: 3,
        ok: false,
        error: expect.stringContaining("nonexistent"),
      });
    });
  });

  describe("startup", () => {
    it("exits non-zero with clean stderr when the config has no default export", async () => {
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
          if (proc.exitCode === null) {
            proc.kill("SIGKILL");
          }
        }, 10_000);
      });
      expect(exitCode).not.toBe(0);
      // "Clean": no V8 stack trace.
      expect(stderr).not.toMatch(/\s+at [^\s]+ \(/);
      expect(stderr.toLowerCase()).toMatch(/default|export/);
    });
  });
});
