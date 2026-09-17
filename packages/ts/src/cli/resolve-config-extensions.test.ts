import { beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "../core.js";
import { resolveConfigsExtensionSpecs } from "../resolve-config-extensions.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("../core.js");
vi.mock("../resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("../resolve-config-extensions.js")>(
    "../resolve-config-extensions.js",
  )),
  resolveConfigsExtensionSpecs: vi.fn(),
}));

describe("withResolvedExtensions", () => {
  const configPathsFromArgv = vi.fn<(argv: string[]) => string[]>();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getCore).mockReturnValue({ configPathsFromArgv } as never);
    configPathsFromArgv.mockReturnValue(["/x/.dirsql.toml"]);
  });

  it("passes `init` through untouched without scanning or resolving", async () => {
    const argv = ["init", "--root", "."];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(configPathsFromArgv).not.toHaveBeenCalled();
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("scans the whole argv for config paths", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    const argv = ["query", "SELECT 1", "-c", "/frag/dirsql.toml"];
    await withResolvedExtensions(argv);
    expect(configPathsFromArgv).toHaveBeenCalledWith(argv);
  });

  it("passes native configs through untouched (interpret resolves them)", async () => {
    configPathsFromArgv.mockReturnValue([
      "cfg.py",
      "cfg.js",
      "cfg.mjs",
      "cfg.cjs",
    ]);
    const argv = ["--config", "dirsql.config.mjs"];
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([]);
  });

  it("drops native configs but resolves the TOML ones", async () => {
    configPathsFromArgv.mockReturnValue([
      "cfg.py",
      "/frag/dirsql.toml",
      "other.cjs",
    ]);
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["-c", "cfg.py", "-c", "/frag/dirsql.toml"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("consults the resolver with every path, missing configs included", async () => {
    // The resolver hands each config to the core, which skips the ones it
    // could not read.
    configPathsFromArgv.mockReturnValue(["/a/.dirsql.toml", "/b/.dirsql.toml"]);
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions([
      "-c",
      "/a/.dirsql.toml",
      "-c",
      "/b/.dirsql.toml",
    ]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/a/.dirsql.toml",
      "/b/.dirsql.toml",
    ]);
  });

  it("passes through when the resolver does not intervene", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/x/.dirsql.toml",
    ]);
  });

  it("appends --extension flags for resolved specs", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
      { path: "R:sqlite_vec", entrypoint: "sqlite3_vec_init" },
      { path: "R:ext/local.so", entrypoint: undefined },
    ]);
    const out = await withResolvedExtensions(["--config", "/cfg/.dirsql.toml"]);
    expect(out).toEqual([
      "--config",
      "/cfg/.dirsql.toml",
      "--extension",
      "R:sqlite_vec::sqlite3_vec_init",
      "--extension",
      "R:ext/local.so",
    ]);
  });
});
