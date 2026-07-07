import { describe, expect, it, vi } from "vitest";
import { resolveConfigExtensionSpecs } from "../resolve-config-extensions.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("../resolve-config-extensions.js", async () => ({
  ...(await vi.importActual<typeof import("../resolve-config-extensions.js")>(
    "../resolve-config-extensions.js",
  )),
  resolveConfigExtensionSpecs: vi.fn(),
}));

describe("withResolvedExtensions", () => {
  it("passes `init` through untouched without consulting the resolver", () => {
    const argv = ["init", "--root", "."];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigExtensionSpecs).not.toHaveBeenCalled();
  });

  it("passes a native config through untouched (interpret resolves it)", () => {
    const argv = ["--config", "dirsql.config.mjs"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigExtensionSpecs).not.toHaveBeenCalled();
  });

  it("passes through when the resolver does not intervene", () => {
    vi.mocked(resolveConfigExtensionSpecs).mockReturnValue(null);
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigExtensionSpecs).toHaveBeenCalledWith("/x/.dirsql.toml");
  });

  it("appends --extension flags for resolved specs", () => {
    vi.mocked(resolveConfigExtensionSpecs).mockReturnValue([
      { path: "R:sqlite_vec", entrypoint: "sqlite3_vec_init" },
      { path: "R:ext/local.so", entrypoint: undefined },
    ]);
    const out = withResolvedExtensions(["--config", "/cfg/.dirsql.toml"]);
    expect(out).toEqual([
      "--config",
      "/cfg/.dirsql.toml",
      "--extension",
      "R:sqlite_vec::sqlite3_vec_init",
      "--extension",
      "R:ext/local.so",
    ]);
  });

  it("reads the `--config=X` form", () => {
    vi.mocked(resolveConfigExtensionSpecs).mockReturnValue([
      { path: "R:pkg", entrypoint: undefined },
    ]);
    const out = withResolvedExtensions(["--config=/c/.dirsql.toml"]);
    expect(out).toEqual(["--config=/c/.dirsql.toml", "--extension", "R:pkg"]);
    expect(resolveConfigExtensionSpecs).toHaveBeenCalledWith("/c/.dirsql.toml");
  });

  it("defaults to ./.dirsql.toml when no --config is given", () => {
    vi.mocked(resolveConfigExtensionSpecs).mockReturnValue([
      { path: "R:pkg", entrypoint: undefined },
    ]);
    const out = withResolvedExtensions(["--port", "9000"]);
    expect(out).toEqual(["--port", "9000", "--extension", "R:pkg"]);
    expect(resolveConfigExtensionSpecs).toHaveBeenCalledWith("./.dirsql.toml");
  });

  it("treats a bare trailing --config as an empty path (default file check)", () => {
    // `--config` with no following value: `argv[i + 1]` is undefined -> "".
    vi.mocked(resolveConfigExtensionSpecs).mockReturnValue(null);
    const argv = ["--config"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(resolveConfigExtensionSpecs).toHaveBeenCalledWith("");
  });
});
