import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { main } from "./main.js";
import { runCli } from "./run-cli.js";

vi.mock("./main.js");

describe("runCli", () => {
  let write: ReturnType<typeof vi.fn>;
  let exit: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    write = vi.fn();
    exit = vi.fn();
    vi.stubGlobal("process", { ...process, stderr: { write }, exit });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("invokes main", () => {
    vi.mocked(main).mockResolvedValue(undefined);
    runCli();
    expect(main).toHaveBeenCalledOnce();
  });

  it("writes an Error's message to stderr and exits 1 on rejection", async () => {
    vi.mocked(main).mockRejectedValue(new Error("boom"));
    runCli();
    await vi.waitFor(() => expect(exit).toHaveBeenCalledWith(1));
    expect(write).toHaveBeenCalledWith("dirsql: boom\n");
  });

  it("stringifies a non-Error rejection", async () => {
    vi.mocked(main).mockRejectedValue("nope");
    runCli();
    await vi.waitFor(() => expect(exit).toHaveBeenCalledWith(1));
    expect(write).toHaveBeenCalledWith("dirsql: nope\n");
  });
});
