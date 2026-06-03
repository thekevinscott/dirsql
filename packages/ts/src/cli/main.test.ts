// Unit tests for `main`.

import type { SpawnSyncReturns } from "node:child_process";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type MainDeps, main } from "./main.js";

const TEST_PID = 42;

function fakeResult<T extends Partial<SpawnSyncReturns<Buffer>>>(
  overrides: T,
): SpawnSyncReturns<Buffer> {
  return {
    pid: TEST_PID,
    stdout: Buffer.from(""),
    stderr: Buffer.from(""),
    output: [],
    status: 0,
    signal: null,
    ...overrides,
  } as SpawnSyncReturns<Buffer>;
}

describe("main", () => {
  let exit: ReturnType<typeof vi.fn>;
  let kill: ReturnType<typeof vi.fn>;

  beforeEach(() => {
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

  afterEach(() => vi.unstubAllGlobals());

  const fakeDie = (msg: string): never => {
    throw new Error(`DIE: ${msg}`);
  };

  it("spawns the resolved binary with the argv and exits with the spawn status", () => {
    const spawn = vi.fn().mockReturnValue(fakeResult({ status: 0 }));
    const deps: MainDeps = {
      resolve: () => "/bin/dirsql",
      spawn,
      dieFn: fakeDie,
    };

    expect(() => main(["--version"], deps)).toThrow("EXIT_0");
    expect(spawn).toHaveBeenCalledWith("/bin/dirsql", ["--version"]);
    expect(exit).toHaveBeenCalledWith(0);
  });

  it("falls back to exit code 1 when spawn returns a null status", () => {
    const spawn = vi.fn().mockReturnValue(fakeResult({ status: null }));
    const deps: MainDeps = {
      resolve: () => "/bin/dirsql",
      spawn,
      dieFn: fakeDie,
    };

    expect(() => main([], deps)).toThrow("EXIT_1");
    expect(exit).toHaveBeenCalledWith(1);
  });

  it("calls dieFn with the spawn error message when spawn reports an error", () => {
    const spawn = vi
      .fn()
      .mockReturnValue(
        fakeResult({ status: null, error: new Error("spawn ENOENT") }),
      );
    const dieFn = vi
      .fn()
      .mockImplementation(fakeDie) as unknown as MainDeps["dieFn"];
    const deps: MainDeps = {
      resolve: () => "/bin/dirsql",
      spawn,
      dieFn,
    };

    expect(() => main([], deps)).toThrow("DIE: spawn ENOENT");
    expect(dieFn).toHaveBeenCalledWith("spawn ENOENT", 1);
  });

  it("re-raises the spawned process's signal against the current pid before exiting", () => {
    const spawn = vi
      .fn()
      .mockReturnValue(fakeResult({ status: 0, signal: "SIGINT" }));
    const deps: MainDeps = {
      resolve: () => "/bin/dirsql",
      spawn,
      dieFn: fakeDie,
    };

    expect(() => main([], deps)).toThrow("EXIT_0");
    expect(kill).toHaveBeenCalledWith(TEST_PID, "SIGINT");
  });
});
