import { existsSync, readFileSync } from "node:fs";
import { parse as parseToml } from "smol-toml";
import { describe, expect, it, vi } from "vitest";
import {
  resolveConfigExtensionSpecs,
  resolveConfigsExtensionSpecs,
} from "./resolve-config-extensions.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  readFileSync: vi.fn(() => ""),
}));
vi.mock("smol-toml", async () => ({
  ...(await vi.importActual<typeof import("smol-toml")>("smol-toml")),
  parse: vi.fn(),
}));
vi.mock("./resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-extension.js")>(
    "./resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));

function stubConfig(doc: unknown, exists = true) {
  vi.mocked(existsSync).mockReturnValue(exists);
  vi.mocked(parseToml).mockReturnValue(doc as ReturnType<typeof parseToml>);
}

describe("resolveConfigExtensionSpecs", () => {
  it("returns null when the config file does not exist", () => {
    vi.mocked(existsSync).mockReturnValue(false);
    expect(resolveConfigExtensionSpecs("/nope/.dirsql.toml")).toBeNull();
    expect(readFileSync).not.toHaveBeenCalled();
  });

  it("returns null when the TOML fails to parse", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(parseToml).mockImplementation(() => {
      throw new Error("bad toml");
    });
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when the config declares no extensions", () => {
    stubConfig({ dirsql: { ignore: ["x"] } });
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when the config has no [dirsql] section at all", () => {
    stubConfig({
      table: [{ name: "t", ddl: "CREATE TABLE t (x TEXT)", glob: "*" }],
    });
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when `extension` is not an array", () => {
    stubConfig({ dirsql: { extension: "not-a-list" } });
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
  });

  it("returns null when all extension paths are literal (no package name)", () => {
    stubConfig({ dirsql: { extension: [{ path: "ext/a.so" }] } });
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("resolves every entry when a path is a bare package name", () => {
    stubConfig({
      dirsql: {
        extension: [
          { path: "sqlite_vec", entrypoint: "sqlite3_vec_init" },
          { path: "ext/local.so" },
        ],
      },
    });
    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "R:sqlite_vec", entrypoint: "sqlite3_vec_init" },
      { path: "R:ext/local.so", entrypoint: undefined },
    ]);
    expect(resolveExtensionPath).toHaveBeenCalledWith(
      "sqlite_vec",
      "/cfg",
      true,
    );
    expect(resolveExtensionPath).toHaveBeenCalledWith(
      "ext/local.so",
      "/cfg",
      true,
    );
  });

  it("normalizes a non-string entrypoint to undefined", () => {
    stubConfig({
      dirsql: { extension: [{ path: "sqlite_vec", entrypoint: 42 }] },
    });
    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });

  it("skips a non-string path in the package-name probe", () => {
    // An entry whose `path` isn't a string is not treated as a package name;
    // a sibling bare name still triggers resolution of every entry.
    stubConfig({
      dirsql: { extension: [{ path: 42 }, { path: "sqlite_vec" }] },
    });
    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "R:42", entrypoint: undefined },
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });
});

describe("resolveConfigsExtensionSpecs", () => {
  it("returns null for an empty list", () => {
    vi.clearAllMocks();
    expect(resolveConfigsExtensionSpecs([])).toBeNull();
    expect(existsSync).not.toHaveBeenCalled();
  });

  it("returns null when no config uses a bare package name", () => {
    vi.clearAllMocks();
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(parseToml)
      .mockReturnValueOnce({
        dirsql: { extension: [{ path: "ext/a.so" }] },
      } as ReturnType<typeof parseToml>)
      .mockReturnValueOnce({
        name: "t",
        table: [{ ddl: "CREATE TABLE t (x TEXT)", glob: "*" }],
      } as ReturnType<typeof parseToml>);
    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toBeNull();
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("resolves every config in order when one uses a bare package name", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(parseToml)
      .mockReturnValueOnce({
        dirsql: { extension: [{ path: "ext/a.so" }] },
      } as ReturnType<typeof parseToml>)
      .mockReturnValueOnce({
        dirsql: { extension: [{ path: "sqlite_vec" }] },
      } as ReturnType<typeof parseToml>);
    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toEqual([
      { path: "R:ext/a.so", entrypoint: undefined },
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });

  it("skips a missing config but resolves the rest", () => {
    vi.mocked(existsSync).mockReturnValueOnce(false).mockReturnValueOnce(true);
    vi.mocked(parseToml).mockReturnValueOnce({
      dirsql: { extension: [{ path: "sqlite_vec" }] },
    } as ReturnType<typeof parseToml>);
    expect(resolveConfigsExtensionSpecs(["/missing.toml", "/b.toml"])).toEqual([
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });
});
