import { afterEach, describe, expect, it, vi } from "vitest";
import { hasBareName } from "./has-bare-name.js";
import { type Toml, loadExtensionEntries } from "./load-extension-entries.js";
import { resolveConfigExtensionSpecs } from "./resolve-config-extension-specs.js";
import { resolveEntries } from "./resolve-entries.js";

vi.mock("./has-bare-name.js", async () => ({
  ...(await vi.importActual<typeof import("./has-bare-name.js")>(
    "./has-bare-name.js",
  )),
  hasBareName: vi.fn(),
}));
vi.mock("./load-extension-entries.js", async () => ({
  ...(await vi.importActual<typeof import("./load-extension-entries.js")>(
    "./load-extension-entries.js",
  )),
  loadExtensionEntries: vi.fn(),
}));
vi.mock("./resolve-entries.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-entries.js")>(
    "./resolve-entries.js",
  )),
  resolveEntries: vi.fn(() => []),
}));

function loaded(base: string, entries: Toml[]) {
  return { entries, base };
}

afterEach(() => vi.resetAllMocks());

describe("resolveConfigExtensionSpecs", () => {
  it("returns null when the config yields no entries", () => {
    vi.mocked(loadExtensionEntries).mockReturnValue(null);

    expect(resolveConfigExtensionSpecs("/nope/.dirsql.toml")).toBeNull();
    expect(loadExtensionEntries).toHaveBeenCalledWith("/nope/.dirsql.toml");
    expect(hasBareName).not.toHaveBeenCalled();
    expect(resolveEntries).not.toHaveBeenCalled();
  });

  it("returns null when all extension paths are literal (no package name)", () => {
    vi.mocked(loadExtensionEntries).mockReturnValue(
      loaded("/x", [{ path: "ext/a.so" }]),
    );
    vi.mocked(hasBareName).mockReturnValue(false);

    expect(resolveConfigExtensionSpecs("/x/.dirsql.toml")).toBeNull();
    expect(hasBareName).toHaveBeenCalledWith([{ path: "ext/a.so" }]);
    expect(resolveEntries).not.toHaveBeenCalled();
  });

  it("resolves every entry against the config's dir when a path is a bare name", () => {
    const entries = [{ path: "sqlite_vec" }, { path: "ext/local.so" }];
    vi.mocked(loadExtensionEntries).mockReturnValue(loaded("/cfg", entries));
    vi.mocked(hasBareName).mockReturnValue(true);
    vi.mocked(resolveEntries).mockReturnValue([
      { path: "/cfg/vec0.so", entrypoint: "sqlite3_vec_init" },
    ]);

    expect(resolveConfigExtensionSpecs("/cfg/.dirsql.toml")).toEqual([
      { path: "/cfg/vec0.so", entrypoint: "sqlite3_vec_init" },
    ]);
    expect(resolveEntries).toHaveBeenCalledWith(entries, "/cfg");
  });
});
