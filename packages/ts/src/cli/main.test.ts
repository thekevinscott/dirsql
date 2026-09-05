import { mainInProcess } from "bin-shim";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { die } from "./die.js";
import { keepSignalsFatal } from "./keep-signals-fatal.js";
import { main } from "./main.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";
import { resolveRunCli } from "./resolve-run-cli.js";

vi.mock("bin-shim");
vi.mock("./die.js");
vi.mock("./keep-signals-fatal.js");
vi.mock("./resolve-run-cli.js");
vi.mock("./resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-config-extensions.js")>(
    "./resolve-config-extensions.js",
  )),
  withResolvedExtensions: vi.fn(async (argv: string[]) => argv),
}));

describe("main", () => {
  let exit: ReturnType<typeof vi.fn>;
  const runCli = vi.fn(() => 0);

  beforeEach(() => {
    vi.mocked(resolveRunCli).mockReturnValue(runCli);
    vi.mocked(withResolvedExtensions).mockImplementation(async (argv) => argv);
    vi.mocked(mainInProcess).mockResolvedValue(0);
    vi.mocked(die).mockImplementation(((msg: string) => {
      throw new Error(`DIE: ${msg}`);
    }) as typeof die);
    exit = vi.fn().mockImplementation((code: number) => {
      throw new Error(`EXIT_${code}`);
    });
    vi.stubGlobal("process", { ...process, exit });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.resetAllMocks();
  });

  it("runs the CLI in-process and exits with its code", async () => {
    vi.mocked(mainInProcess).mockResolvedValue(23);

    await expect(main(["query", "SELECT 1"])).rejects.toThrow("EXIT_23");
    expect(withResolvedExtensions).toHaveBeenCalledWith(["query", "SELECT 1"]);
    expect(mainInProcess).toHaveBeenCalledWith(
      expect.objectContaining({
        argv: ["query", "SELECT 1"],
        binaryName: "dirsql",
        runCli,
      }),
    );
  });

  it("passes the resolver's augmented argv, not the raw argv", async () => {
    vi.mocked(withResolvedExtensions).mockResolvedValue([
      "query",
      "SELECT 1",
      "--extension",
      "/r/vec0",
    ]);

    await expect(main(["query", "SELECT 1"])).rejects.toThrow("EXIT_0");
    expect(mainInProcess).toHaveBeenCalledWith(
      expect.objectContaining({
        argv: ["query", "SELECT 1", "--extension", "/r/vec0"],
      }),
    );
  });

  it("defaults argv to process.argv minus the node/script slots", async () => {
    vi.stubGlobal("process", {
      ...process,
      argv: ["node", "dirsql", "--version"],
      exit,
    });

    await expect(main()).rejects.toThrow("EXIT_0");
    expect(withResolvedExtensions).toHaveBeenCalledWith(["--version"]);
  });

  it("makes the signals fatal before running the CLI", async () => {
    // Ordering is the whole fix: signal-hook chains to the handler installed
    // BEFORE it, and bare Node leaves SIG_DFL, which it does not emulate. A
    // listener registered after the core's would not keep Ctrl-C fatal.
    const order: string[] = [];
    vi.mocked(keepSignalsFatal).mockImplementation(() => {
      order.push("keepSignalsFatal");
    });
    vi.mocked(mainInProcess).mockImplementation(async () => {
      order.push("runCli");
      return 0;
    });

    await expect(main([])).rejects.toThrow("EXIT_0");
    expect(order).toEqual(["keepSignalsFatal", "runCli"]);
  });

  it("dies with the message when runCli cannot be resolved", async () => {
    vi.mocked(resolveRunCli).mockImplementation(() => {
      throw new Error("no prebuilt addon for linux-x64");
    });

    await expect(main([])).rejects.toThrow("DIE: no prebuilt addon");
    expect(mainInProcess).not.toHaveBeenCalled();
  });

  it("stringifies a non-Error rejection", async () => {
    vi.mocked(mainInProcess).mockRejectedValue("boom");

    await expect(main([])).rejects.toThrow("DIE: boom");
  });
});
