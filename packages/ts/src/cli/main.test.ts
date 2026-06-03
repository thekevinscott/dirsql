// Unit tests for `main`.

import { spawnSync } from "node:child_process";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { die } from "./die.js";
import { interpret } from "./interpret/index.js";
import { main } from "./main.js";
import { resolveBinary } from "./resolveBinary.js";

vi.mock("./resolveBinary.js");
vi.mock("./die.js");
vi.mock("./interpret/index.js");
vi.mock("node:child_process");

const TEST_PID = 42;

type SpawnResult = ReturnType<typeof spawnSync>;

function fakeResult(overrides: Record<string, unknown>): SpawnResult {
  return {
    pid: TEST_PID,
    stdout: Buffer.from(""),
    stderr: Buffer.from(""),
    output: [],
    status: 0,
    signal: null,
    ...overrides,
  } as unknown as SpawnResult;
}

describe("main", () => {
  let exit: ReturnType<typeof vi.fn>;
  let kill: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.mocked(resolveBinary).mockReturnValue("/bin/dirsql");
    vi.mocked(die).mockImplementation(((msg: string) => {
      throw new Error(`DIE: ${msg}`);
    }) as typeof die);
    exit = vi.fn().mockImplementation((code: number) => {
      throw new Error(`EXIT_${code}`);
    });
    kill = vi.fn();
    vi.stubGlobal("process", {
      ...process,
      exit,
      kill,
      pid: TEST_PID,
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("spawns the resolved binary with the argv and exits with the spawn status", async () => {
    vi.mocked(spawnSync).mockReturnValue(fakeResult({ status: 0 }));

    await expect(main(["--version"])).rejects.toThrow("EXIT_0");
    expect(spawnSync).toHaveBeenCalledWith("/bin/dirsql", ["--version"], {
      stdio: "inherit",
    });
    expect(exit).toHaveBeenCalledWith(0);
  });

  it("falls back to exit code 1 when spawn returns a null status", async () => {
    vi.mocked(spawnSync).mockReturnValue(fakeResult({ status: null }));

    await expect(main([])).rejects.toThrow("EXIT_1");
    expect(exit).toHaveBeenCalledWith(1);
  });

  it("calls die with the spawn error message when spawn reports an error", async () => {
    vi.mocked(spawnSync).mockReturnValue(
      fakeResult({ status: null, error: new Error("spawn ENOENT") }),
    );

    await expect(main([])).rejects.toThrow("DIE: spawn ENOENT");
    expect(die).toHaveBeenCalledWith("spawn ENOENT", 1);
  });

  it("re-raises the spawned process's signal against the current pid before exiting", async () => {
    vi.mocked(spawnSync).mockReturnValue(
      fakeResult({ status: 0, signal: "SIGINT" }),
    );

    await expect(main([])).rejects.toThrow("EXIT_0");
    expect(kill).toHaveBeenCalledWith(TEST_PID, "SIGINT");
  });

  it("defaults argv to process.argv.slice(2) when called with no args", async () => {
    vi.mocked(spawnSync).mockReturnValue(fakeResult({ status: 0 }));
    vi.stubGlobal("process", {
      ...process,
      argv: ["node", "dirsql", "--help"],
      exit,
      kill,
      pid: TEST_PID,
    });

    await expect(main()).rejects.toThrow("EXIT_0");
    expect(spawnSync).toHaveBeenCalledWith("/bin/dirsql", ["--help"], {
      stdio: "inherit",
    });
  });

  describe("when argv[0] is 'interpret'", () => {
    it("dispatches to the interpret helper and exits with its return code", async () => {
      vi.mocked(interpret).mockResolvedValue(0);

      await expect(main(["interpret", "config.mjs"])).rejects.toThrow("EXIT_0");
      expect(interpret).toHaveBeenCalledWith("config.mjs");
      expect(spawnSync).not.toHaveBeenCalled();
      expect(resolveBinary).not.toHaveBeenCalled();
    });

    it("propagates a non-zero interpret exit code", async () => {
      vi.mocked(interpret).mockResolvedValue(2);

      await expect(main(["interpret", "bad.mjs"])).rejects.toThrow("EXIT_2");
    });

    it("passes the empty string when no config path follows", async () => {
      vi.mocked(interpret).mockResolvedValue(1);

      await expect(main(["interpret"])).rejects.toThrow("EXIT_1");
      expect(interpret).toHaveBeenCalledWith("");
    });

    it("does not intercept when 'interpret' is not the first arg", async () => {
      vi.mocked(spawnSync).mockReturnValue(fakeResult({ status: 0 }));

      await expect(
        main(["--verbose", "interpret", "config.mjs"]),
      ).rejects.toThrow("EXIT_0");
      expect(interpret).not.toHaveBeenCalled();
      expect(spawnSync).toHaveBeenCalled();
    });
  });
});
