// Integration tests for the `dirsql interpret` entry point.
//
// In-process: drives `interpret()` directly with a real `DirSQL` (loadApp is
// mocked to return it), stubbing only the I/O boundaries (process.stdin /
// stdout / cwd). The cheaper, CI-running mirror of the subprocess e2e tests
// in `tests/e2e/interpret.test.ts`.

import { mkdtemp, realpath, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Readable } from "node:stream";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { interpret } from "../../src/cli/interpret/interpret.js";
import { loadApp } from "../../src/cli/interpret/load-app.js";

vi.mock("../../src/cli/interpret/load-app.js", () => ({ loadApp: vi.fn() }));

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
    vi.mocked(loadApp).mockResolvedValue(
      new DirSQL({
        tables: [
          {
            ddl: "CREATE TABLE papers (title TEXT)",
            glob: "**/meta.json",
            extract: () => [],
          },
        ],
      }),
    );

    const rc = await interpret("/whatever.mjs");

    expect(rc).toBe(0);
    const handshake = JSON.parse(stdout.join("").split("\n")[0]);
    expect(handshake.state.root).toBe("/fake/cwd");
  });

  it("rejects a config that sets config=", async () => {
    // A valid TOML so the handshake path (toJSON) would otherwise succeed --
    // the rejection must come from the loader recognizing the nested config.
    const dir = await realpath(await mkdtemp(join(tmpdir(), "dirsql-nested-")));
    const toml = join(dir, "nested.dirsql.toml");
    await writeFile(
      toml,
      '[[table]]\nddl = "CREATE TABLE papers (title TEXT)"\nglob = "**/meta.json"\n',
    );
    vi.mocked(loadApp).mockResolvedValue(new DirSQL({ config: toml }));

    const rc = await interpret("/whatever.mjs");

    expect(rc).not.toBe(0);
    expect(stderr.join("").toLowerCase()).toMatch(/config/);
  });
});
