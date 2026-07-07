import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { die } from "./die.js";

describe("die", () => {
  let stderrWrite: ReturnType<typeof vi.fn>;
  let exit: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    stderrWrite = vi.fn();
    exit = vi.fn().mockImplementation((code: number) => {
      throw new Error(`EXIT_${code}`);
    });
    vi.stubGlobal("process", {
      ...process,
      stderr: { write: stderrWrite },
      exit,
    });
  });

  afterEach(() => vi.unstubAllGlobals());

  it("writes a 'dirsql: ' prefixed message to stderr and exits with the given code", () => {
    expect(() => die("boom", 2)).toThrow("EXIT_2");
    expect(stderrWrite).toHaveBeenCalledWith("dirsql: boom\n");
    expect(exit).toHaveBeenCalledWith(2);
  });

  it("defaults the exit code to 1", () => {
    expect(() => die("boom")).toThrow("EXIT_1");
    expect(exit).toHaveBeenCalledWith(1);
  });
});
