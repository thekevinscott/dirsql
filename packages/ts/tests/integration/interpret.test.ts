// Integration tests for the `dirsql interpret` entry point.
//
// In-process: drives the real `interpret()` against real config fixtures (real
// `loadApp` + real `DirSQL`), mocking only Node built-ins -- the stdio streams
// (so the handshake is captured and stdin ends immediately) and `process.cwd`.
// The cheaper, CI-running mirror of the subprocess e2e tests in
// `tests/e2e/interpret.test.ts` (no spawned process, no HTTP server).

import { dirname, join } from "node:path";
import { Readable } from "node:stream";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { interpret } from "../../src/cli/interpret/interpret.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
// Reuse the e2e fixtures: real `.mjs` configs that `import { DirSQL } from
// "dirsql"`. They live inside the package tree so the self-reference resolves.
const FIXTURES = join(__dirname, "..", "e2e", "__fixtures__", "interpret");

describe("dirsql interpret (integration)", () => {
  let stdout: string[];
  let stderr: string[];
  let origStdin: NodeJS.ReadStream;

  beforeEach(() => {
    stdout = [];
    stderr = [];
    vi.spyOn(process.stdout, "write").mockImplementation((c: unknown) => {
      stdout.push(String(c));
      return true;
    });
    vi.spyOn(process.stderr, "write").mockImplementation((c: unknown) => {
      stderr.push(String(c));
      return true;
    });
    // Empty stdin that ends immediately, so interpret()'s read loop exits
    // right after writing the handshake.
    origStdin = process.stdin;
    Object.defineProperty(process, "stdin", {
      value: Readable.from([]),
      configurable: true,
    });
  });

  afterEach(() => {
    Object.defineProperty(process, "stdin", {
      value: origStdin,
      configurable: true,
    });
    vi.restoreAllMocks();
  });

  it("defaults root to the process cwd when the config omits root", async () => {
    vi.spyOn(process, "cwd").mockReturnValue("/fake/cwd");

    const rc = await interpret(join(FIXTURES, "dirsql.config-no-root.mjs"));

    expect(rc).toBe(0);
    const handshake = JSON.parse(stdout.join("").split("\n")[0]);
    expect(handshake.state.root).toBe("/fake/cwd");
  });

  it("rejects a config that sets config=", async () => {
    const rc = await interpret(join(FIXTURES, "dirsql.config-nested.mjs"));

    expect(rc).not.toBe(0);
    expect(stderr.join("").toLowerCase()).toMatch(/config/);
  });
});
