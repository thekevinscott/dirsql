// Unit tests for `withResolvedExtensions`.
//
// Effectful collaborators are mocked: `node:fs` (existsSync/readFileSync) and
// `smol-toml` (parse) via `vi.mock`, and `resolveExtensionPath` via an anchored
// factory (the pure `isBareName` stays real). No real files or packages.

import { existsSync, readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import { describe, expect, it, vi } from "vitest";
import { resolveExtensionPath } from "../resolve-extension.js";
import { withResolvedExtensions } from "./resolve-config-extensions.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  readFileSync: vi.fn(() => ""),
}));
vi.mock("smol-toml", async () => ({
  ...(await vi.importActual<typeof import("smol-toml")>("smol-toml")),
  parse: vi.fn(),
}));
vi.mock("../resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("../resolve-extension.js")>(
    "../resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));

function stubConfig(doc: unknown, exists = true) {
  vi.mocked(existsSync).mockReturnValue(exists);
  vi.mocked(parseToml).mockReturnValue(doc as ReturnType<typeof parseToml>);
}

describe("withResolvedExtensions", () => {
  it("passes `init` through untouched without reading any file", () => {
    const argv = ["init", "--root", "."];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).not.toHaveBeenCalled();
  });

  it("passes a native config through untouched (interpret resolves it)", () => {
    const argv = ["--config", "dirsql.config.mjs"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).not.toHaveBeenCalled();
  });

  it("passes through when the config file does not exist", () => {
    vi.mocked(existsSync).mockReturnValue(false);
    const argv = ["--config", "/nope/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
  });

  it("passes through when the TOML fails to parse", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(parseToml).mockImplementation(() => {
      throw new Error("bad toml");
    });
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
  });

  it("passes through when the config declares no extensions", () => {
    stubConfig({ dirsql: { ignore: ["x"] } });
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
  });

  it("passes through when the config has no [dirsql] section at all", () => {
    stubConfig({ table: [{ ddl: "CREATE TABLE t (x TEXT)", glob: "*" }] });
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
  });

  it("passes through when all extension paths are literal (no package name)", () => {
    stubConfig({ dirsql: { extension: [{ path: "ext/a.so" }] } });
    const argv = ["--config", "/x/.dirsql.toml"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("appends --extension flags when a path is a bare package name", () => {
    stubConfig({
      dirsql: {
        extension: [
          { path: "sqlite_vec", entrypoint: "sqlite3_vec_init" },
          { path: "ext/local.so" },
        ],
      },
    });
    const out = withResolvedExtensions(["--config", "/cfg/.dirsql.toml"]);
    expect(out).toEqual([
      "--config",
      "/cfg/.dirsql.toml",
      "--extension",
      "R:sqlite_vec::sqlite3_vec_init",
      "--extension",
      "R:ext/local.so",
    ]);
    // Config entries resolve against the config's parent dir.
    expect(resolveExtensionPath).toHaveBeenCalledWith(
      "sqlite_vec",
      "/cfg",
      true,
    );
  });

  it("reads the `--config=X` form", () => {
    stubConfig({ dirsql: { extension: [{ path: "pkg" }] } });
    const out = withResolvedExtensions(["--config=/c/.dirsql.toml"]);
    expect(out).toEqual(["--config=/c/.dirsql.toml", "--extension", "R:pkg"]);
  });

  it("defaults to ./.dirsql.toml when no --config is given", () => {
    stubConfig({ dirsql: { extension: [{ path: "pkg" }] } });
    const out = withResolvedExtensions(["--port", "9000"]);
    expect(out).toEqual(["--port", "9000", "--extension", "R:pkg"]);
    expect(existsSync).toHaveBeenCalledWith("./.dirsql.toml");
  });

  it("treats a bare trailing --config as an empty path (default file check)", () => {
    // `--config` with no following value: `argv[i + 1]` is undefined -> "".
    vi.mocked(existsSync).mockReturnValue(false);
    const argv = ["--config"];
    expect(withResolvedExtensions(argv)).toBe(argv);
    expect(existsSync).toHaveBeenCalledWith("");
  });
});
