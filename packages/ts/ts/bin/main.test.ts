import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./resolveBinary", () => ({ resolveBinary: vi.fn() }));
vi.mock("node:child_process", () => ({ spawnSync: vi.fn() }));

import { spawnSync } from "node:child_process";
import { main } from "./main.js";
import { resolveBinary } from "./resolveBinary.js";

const spawnSyncMock = vi.mocked(spawnSync);
const resolveBinaryMock = vi.mocked(resolveBinary);

afterEach(() => vi.restoreAllMocks());

function stubExit() {
  return vi.spyOn(process, "exit").mockImplementation(((
    code?: string | number | null,
  ) => {
    throw new Error(`exit:${code}`);
  }) as typeof process.exit);
}

function stubStderr() {
  vi.spyOn(process.stderr, "write").mockReturnValue(true);
}

function stubKill() {
  return vi.spyOn(process, "kill").mockImplementation(() => true);
}

describe("main", () => {
  describe("on success", () => {
    it("resolves the binary, spawns it with argv, and exits with the child's status", () => {
      resolveBinaryMock.mockReturnValue("/real/dirsql");
      spawnSyncMock.mockReturnValue({
        status: 0,
        signal: null,
        pid: 123,
        output: [],
        stdout: Buffer.from(""),
        stderr: Buffer.from(""),
      } as never);
      const exit = stubExit();

      expect(() => main(["--help"])).toThrow("exit:0");
      expect(resolveBinaryMock).toHaveBeenCalledTimes(1);
      expect(spawnSyncMock).toHaveBeenCalledWith("/real/dirsql", ["--help"], {
        stdio: "inherit",
      });
      expect(exit).toHaveBeenCalledWith(0);
    });
  });

  describe("when the child exits non-zero", () => {
    it("forwards the exit code", () => {
      resolveBinaryMock.mockReturnValue("/real/dirsql");
      spawnSyncMock.mockReturnValue({
        status: 42,
        signal: null,
        pid: 1,
        output: [],
        stdout: Buffer.from(""),
        stderr: Buffer.from(""),
      } as never);
      const exit = stubExit();

      expect(() => main([])).toThrow("exit:42");
      expect(exit).toHaveBeenCalledWith(42);
    });
  });

  describe("when spawn fails to launch the child", () => {
    it("dies with the spawn error message and exits 1", () => {
      resolveBinaryMock.mockReturnValue("/real/dirsql");
      spawnSyncMock.mockReturnValue({
        status: null,
        signal: null,
        pid: 0,
        output: [],
        stdout: Buffer.from(""),
        stderr: Buffer.from(""),
        error: new Error("ENOENT"),
      } as never);
      stubStderr();
      const exit = stubExit();

      expect(() => main([])).toThrow();
      expect(exit).toHaveBeenCalledWith(1);
      expect(process.stderr.write).toHaveBeenCalledWith(
        expect.stringContaining("ENOENT"),
      );
    });
  });

  describe("when the child dies from a signal", () => {
    it("re-raises the same signal against itself", () => {
      resolveBinaryMock.mockReturnValue("/real/dirsql");
      spawnSyncMock.mockReturnValue({
        status: null,
        signal: "SIGINT",
        pid: 1,
        output: [],
        stdout: Buffer.from(""),
        stderr: Buffer.from(""),
      } as never);
      const kill = stubKill();
      const exit = stubExit();

      expect(() => main([])).toThrow();
      expect(kill).toHaveBeenCalledWith(process.pid, "SIGINT");
      expect(exit).toHaveBeenCalledWith(1);
    });
  });

  describe("when argv is omitted", () => {
    it("defaults to process.argv.slice(2)", () => {
      resolveBinaryMock.mockReturnValue("/real/dirsql");
      spawnSyncMock.mockReturnValue({
        status: 0,
        signal: null,
        pid: 1,
        output: [],
        stdout: Buffer.from(""),
        stderr: Buffer.from(""),
      } as never);
      vi.stubGlobal("process", {
        ...process,
        argv: ["node", "bin", "a", "b"],
      });
      stubExit();

      try {
        main();
      } catch {
        // process.exit is stubbed to throw
      }
      expect(spawnSyncMock).toHaveBeenCalledWith(
        "/real/dirsql",
        ["a", "b"],
        expect.any(Object),
      );
    });
  });
});
