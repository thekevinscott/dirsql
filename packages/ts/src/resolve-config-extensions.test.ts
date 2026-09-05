import { afterEach, describe, expect, it, vi } from "vitest";
import { type Toml, loadExtensionEntries } from "./load-extension-entries.js";
import {
  resolveConfigExtensionSpecs,
  resolveConfigsExtensionSpecs,
} from "./resolve-config-extensions.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./load-extension-entries.js", async () => ({
  ...(await vi.importActual<typeof import("./load-extension-entries.js")>(
    "./load-extension-entries.js",
  )),
  loadExtensionEntries: vi.fn(),
}));
vi.mock("./resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-extension.js")>(
    "./resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));

function loaded(base: string, entries: Toml[]) {
  return { entries, base };
}

afterEach(() => {
  vi.mocked(loadExtensionEntries).mockReset();
  vi.mocked(resolveExtensionPath).mockClear();
});

describe("resolveConfigExtensionSpecs", () => {
  it("returns null when the config yields no entries", () => {
    vi.mocked(loadExtensionEntries).mockReturnValue(null);
    expect(resolveConfigExtensionSpecs("/nope/.dirsql.toml")).toBeNull();
    expect(loadExtensionEntries).toHaveBeenCalledWith("/nope/.dirsql.toml");
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("returns null when all extension paths are literal (no package name)", () => {
    vi.mocked(loadExtensionEntries).mockReturnValue(
      loaded("/x", [{ path: "ext/a.so" }]),
    );
    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("resolves every entry when a path is a bare package name", () => {
    vi.mocked(loadExtensionEntries).mockReturnValue(
      loaded("/cfg", [
        { path: "sqlite_vec", entrypoint: "sqlite3_vec_init" },
        { path: "ext/local.so" },
      ]),
    );
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
    vi.mocked(loadExtensionEntries).mockReturnValue(
      loaded("/cfg", [{ path: "sqlite_vec", entrypoint: 42 }]),
    );
    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });

  it("skips a non-string path in the package-name probe", () => {
    // An entry whose `path` isn't a string is not treated as a package name;
    // a sibling bare name still triggers resolution of every entry.
    vi.mocked(loadExtensionEntries).mockReturnValue(
      loaded("/cfg", [{ path: 42 }, { path: "sqlite_vec" }]),
    );
    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "R:42", entrypoint: undefined },
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });
});

describe("resolveConfigsExtensionSpecs", () => {
  it("returns null for an empty list", () => {
    expect(resolveConfigsExtensionSpecs([])).toBeNull();
    expect(loadExtensionEntries).not.toHaveBeenCalled();
  });

  it("returns null when no config uses a bare package name", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(loaded("/a", [{ path: "ext/a.so" }]))
      .mockReturnValueOnce(null);
    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toBeNull();
    expect(resolveExtensionPath).not.toHaveBeenCalled();
  });

  it("resolves every config in order when one uses a bare package name", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(loaded("/a", [{ path: "ext/a.so" }]))
      .mockReturnValueOnce(loaded("/b", [{ path: "sqlite_vec" }]));
    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toEqual([
      { path: "R:ext/a.so", entrypoint: undefined },
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
    expect(resolveExtensionPath).toHaveBeenCalledWith("ext/a.so", "/a", true);
    expect(resolveExtensionPath).toHaveBeenCalledWith("sqlite_vec", "/b", true);
  });

  it("skips a missing config but resolves the rest", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(null)
      .mockReturnValueOnce(loaded("/b", [{ path: "sqlite_vec" }]));
    expect(resolveConfigsExtensionSpecs(["/missing.toml", "/b.toml"])).toEqual([
      { path: "R:sqlite_vec", entrypoint: undefined },
    ]);
  });
});
