import { existsSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resolveConfigsExtensionSpecs } from "../resolve-config-extensions.js";
import { configPathsFromArgv } from "./config-paths-from-argv.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("node:fs");
vi.mock("./config-paths-from-argv.js", () => ({
  configPathsFromArgv: vi.fn(),
}));
vi.mock("../resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("../resolve-config-extensions.js")>(
    "../resolve-config-extensions.js",
  )),
  resolveConfigsExtensionSpecs: vi.fn(),
}));

describe("withResolvedExtensions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(configPathsFromArgv).mockReturnValue(["/x/.dirsql.toml"]);
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
    vi.mocked(configPathsFromArgv).mockReturnValue([
      "cfg.py",
      "cfg.js",
      "cfg.mjs",
      "cfg.cjs",
    ]);
    const argv = ["--config", "dirsql.config.mjs"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).not.toHaveBeenCalled();
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("drops native configs but resolves the TOML ones", async () => {
    vi.mocked(configPathsFromArgv).mockReturnValue([
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

  it("passes through without loading the parser when no config file exists", async () => {
    vi.mocked(existsSync).mockReturnValue(false);
    vi.mocked(configPathsFromArgv).mockReturnValue(["/gone/.dirsql.toml"]);
    const argv = ["--config", "/gone/.dirsql.toml"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).toHaveBeenCalledWith("/gone/.dirsql.toml");
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("consults the resolver with every path when at least one config exists", async () => {
    // The resolver skips missing configs itself; the existsSync guard only
    // keeps the TOML parser off the launch path when NO config exists.
    vi.mocked(existsSync).mockImplementation((p) => p === "/b/.dirsql.toml");
    vi.mocked(configPathsFromArgv).mockReturnValue([
      "/a/.dirsql.toml",
      "/b/.dirsql.toml",
    ]);
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
