import { mainInProcess } from "bin-shim";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { loadNativeCore } from "../load-native-core.js";
import { die } from "./die.js";
import { main, packageVersion } from "./main.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("bin-shim");
vi.mock("../load-native-core.js");
vi.mock("./die.js");
vi.mock("./resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-config-extensions.js")>(
    "./resolve-config-extensions.js",
  )),
  withResolvedExtensions: vi.fn(async (argv: string[]) => argv),
}));

describe("main", () => {
  let exit: ReturnType<typeof vi.fn>;
  let on: ReturnType<typeof vi.fn>;
  const runCli = vi.fn(() => 0);

  beforeEach(() => {
    vi.mocked(loadNativeCore).mockReturnValue({ runCli } as never);
    vi.mocked(withResolvedExtensions).mockImplementation(async (argv) => argv);
    vi.mocked(mainInProcess).mockResolvedValue(0);
    vi.mocked(die).mockImplementation(((msg: string) => {
      throw new Error(`DIE: ${msg}`);
    }) as typeof die);
    exit = vi.fn().mockImplementation((code: number) => {
      throw new Error(`EXIT_${code}`);
    });
    on = vi.fn();
    vi.stubGlobal("process", { ...process, exit, on });
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
        runCli: expect.any(Function),
      }),
    );
  });

  it("hands the addon this package's version, so `--version` reports it", async () => {
    // #958: the addon's own copy of the core carries a literal that only the
    // crates.io release lane rewrites, so left to itself it reports a version
    // nobody installed. Let the shim invoke the callable it was handed, as the
    // real one does, rather than inspecting it.
    vi.mocked(mainInProcess).mockImplementation(
      async ({ argv = [], runCli: forwarded }) => forwarded?.(argv) ?? 0,
    );

    await expect(main(["--version"])).rejects.toThrow("EXIT_0");
    expect(runCli).toHaveBeenCalledExactlyOnceWith(
      ["--version"],
      // The real manifest, read through the default requirer: a specifier that
      // missed it would come back undefined.
      expect.stringMatching(/^\d+\.\d+\.\d+/),
    );
  });

  describe("packageVersion", () => {
    it("reads `version` from the manifest two levels up", () => {
      const requirer = vi.fn(() => ({ version: "4.2.0" }));

      expect(packageVersion(requirer)).toBe("4.2.0");
      expect(requirer).toHaveBeenCalledExactlyOnceWith("../../package.json");
    });

    it("falls back to undefined rather than failing the CLI", () => {
      // Either way the addon's own version stands: wrong, but it starts.
      expect(
        packageVersion(() => {
          throw new Error("Cannot find module '../../package.json'");
        }),
      ).toBeUndefined();
      expect(packageVersion(() => ({ version: 42 }))).toBeUndefined();
    });
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
      on,
    });

    await expect(main()).rejects.toThrow("EXIT_0");
    expect(withResolvedExtensions).toHaveBeenCalledWith(["--version"]);
  });

  it("installs the signal listeners before running the CLI", async () => {
    // Ordering is the whole fix: signal-hook chains to the handler installed
    // BEFORE it, and bare Node leaves SIG_DFL, which it does not emulate. A
    // listener registered after the core's would not keep Ctrl-C fatal.
    const order: string[] = [];
    on.mockImplementation((signal: string) => order.push(`on:${signal}`));
    vi.mocked(mainInProcess).mockImplementation(async () => {
      order.push("runCli");
      return 0;
    });

    await expect(main([])).rejects.toThrow("EXIT_0");
    expect(order).toEqual(["on:SIGINT", "on:SIGTERM", "runCli"]);
  });

  it("exits 130 on SIGINT and 143 on SIGTERM", async () => {
    const handlers: Record<string, () => void> = {};
    on.mockImplementation((signal: string, handler: () => void) => {
      handlers[signal] = handler;
    });

    await expect(main([])).rejects.toThrow("EXIT_0");
    expect(() => handlers.SIGINT?.()).toThrow("EXIT_130");
    expect(() => handlers.SIGTERM?.()).toThrow("EXIT_143");
  });

  it("dies with the message when the addon cannot be loaded", async () => {
    vi.mocked(loadNativeCore).mockImplementation(() => {
      throw new Error("no prebuilt addon for linux-x64");
    });

    await expect(main([])).rejects.toThrow("DIE: no prebuilt addon");
    expect(mainInProcess).not.toHaveBeenCalled();
  });

  it("dies when the addon carries no callable runCli", async () => {
    // A `@dirsql/lib-*` built without the `cli` feature loads fine but has no
    // CLI; say so rather than throwing a bare TypeError.
    vi.mocked(loadNativeCore).mockReturnValue({} as never);

    // The whole message matters: naming the missing `cli` feature is the
    // half that tells someone how to fix their build.
    await expect(main([])).rejects.toThrow(
      "dirsql: the native addon has no callable `runCli` export; " +
        "it was built without the `cli` feature.",
    );
  });

  it("stringifies a non-Error rejection", async () => {
    vi.mocked(mainInProcess).mockRejectedValue("boom");

    await expect(main([])).rejects.toThrow("DIE: boom");
  });
});
