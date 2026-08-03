import { existsSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resolveConfigsExtensionSpecs } from "../resolve-config-extensions.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("node:fs");
vi.mock("../resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("../resolve-config-extensions.js")>(
    "../resolve-config-extensions.js",
  )),
  resolveConfigsExtensionSpecs: vi.fn(),
}));

describe("withResolvedExtensions", () => {
  beforeEach(() => {
    vi.mocked(existsSync).mockReturnValue(true);
  });

  it("passes `init` through untouched without consulting the resolver", async () => {
    const argv = ["init", "--root", "."];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("passes a native config through untouched (interpret resolves it)", async () => {
    const argv = ["--config", "dirsql.config.mjs"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("passes a native short config through untouched", async () => {
    const argv = ["-c", "dirsql.config.mjs"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("passes through without loading the parser when no config file exists", async () => {
    vi.mocked(existsSync).mockReturnValue(false);
    const argv = ["--config", "/gone/.dirsql.toml"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).toHaveBeenCalledWith("/gone/.dirsql.toml");
    expect(resolveConfigsExtensionSpecs).not.toHaveBeenCalled();
  });

  it("consults the resolver with every path when at least one config exists", async () => {
    // The resolver skips missing configs itself; the existsSync guard only
    // keeps the TOML parser off the launch path when NO config exists.
    vi.mocked(existsSync).mockImplementation((p) => p === "/b/.dirsql.toml");
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

  it("reads the `--config=X` form", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
      { path: "R:pkg", entrypoint: undefined },
    ]);
    const out = await withResolvedExtensions(["--config=/c/.dirsql.toml"]);
    expect(out).toEqual(["--config=/c/.dirsql.toml", "--extension", "R:pkg"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/c/.dirsql.toml",
    ]);
  });

  it("reads the short `-c X` form", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
      { path: "R:pkg", entrypoint: undefined },
    ]);
    const out = await withResolvedExtensions(["-c", "/frag/dirsql.toml"]);
    expect(out).toEqual(["-c", "/frag/dirsql.toml", "--extension", "R:pkg"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("reads the short `-c=X` form", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["-c=/frag/dirsql.toml"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("reads the short attached `-cX` form", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["-c/frag/dirsql.toml"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("collects every config flag in argv order", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions([
      "-c",
      "a.toml",
      "--config",
      "b.toml",
      "--config=c.toml",
      "-cd.toml",
    ]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "a.toml",
      "b.toml",
      "c.toml",
      "d.toml",
    ]);
  });

  it("resolves a short-flag config at any position, not just the first --config", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions([
      "query",
      "SELECT 1",
      "--include-default",
      "-c",
      "/frag/dirsql.toml",
    ]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("drops native configs but resolves the TOML ones", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions([
      "-c",
      "cfg.py",
      "-c",
      "/frag/dirsql.toml",
      "-c",
      "other.cjs",
    ]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "/frag/dirsql.toml",
    ]);
  });

  it("defaults to ./.dirsql.toml when no config flag is given", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue([
      { path: "R:pkg", entrypoint: undefined },
    ]);
    const out = await withResolvedExtensions(["--port", "9000"]);
    expect(out).toEqual(["--port", "9000", "--extension", "R:pkg"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "./.dirsql.toml",
    ]);
  });

  it("treats a bare trailing --config as an empty path (no such file)", async () => {
    // `--config` with no following value: `argv[i + 1]` is undefined -> "".
    vi.mocked(existsSync).mockReturnValue(false);
    const argv = ["--config"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).toHaveBeenCalledWith("");
  });

  it("treats a bare trailing -c as an empty path (no such file)", async () => {
    vi.mocked(existsSync).mockReturnValue(false);
    const argv = ["-c"];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).toHaveBeenCalledWith("");
  });

  it("consumes a config value that looks like a flag", async () => {
    // The token after `--config` / `-c` is that flag's value; it is never
    // re-parsed as another config flag.
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["--config", "-cx.toml"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith(["-cx.toml"]);
  });

  it("reads the config value at any argv position", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["-v", "--config", "/x/y", "tail"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith(["/x/y"]);
  });

  it("does not mistake other short flags for config", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    await withResolvedExtensions(["-v", "-x", "val"]);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "./.dirsql.toml",
    ]);
  });

  it("consults the resolver for an empty argv", async () => {
    vi.mocked(resolveConfigsExtensionSpecs).mockReturnValue(null);
    const argv: string[] = [];
    expect(await withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigsExtensionSpecs).toHaveBeenCalledWith([
      "./.dirsql.toml",
    ]);
  });
});
