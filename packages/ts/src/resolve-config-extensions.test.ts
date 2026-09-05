import { afterEach, describe, expect, it, vi } from "vitest";
import { hasBareName } from "./has-bare-name.js";
import { type Toml, loadExtensionEntries } from "./load-extension-entries.js";
import { resolveConfigsExtensionSpecs } from "./resolve-config-extensions.js";
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

describe("resolveConfigsExtensionSpecs", () => {
  it("returns null for an empty list", () => {
    expect(resolveConfigsExtensionSpecs([])).toBeNull();
    expect(loadExtensionEntries).not.toHaveBeenCalled();
  });

  it("returns null when no config uses a bare package name", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(loaded("/a", [{ path: "ext/a.so" }]))
      .mockReturnValueOnce(null);
    vi.mocked(hasBareName).mockReturnValue(false);

    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toBeNull();
    expect(resolveEntries).not.toHaveBeenCalled();
  });

  it("resolves every config in order when one uses a bare package name", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(loaded("/a", [{ path: "ext/a.so" }]))
      .mockReturnValueOnce(loaded("/b", [{ path: "sqlite_vec" }]));
    vi.mocked(hasBareName).mockImplementation(
      (entries) => entries[0]?.path === "sqlite_vec",
    );
    vi.mocked(resolveEntries).mockImplementation((entries, base) =>
      entries.map((e) => ({
        path: `${base}/${e.path}`,
        entrypoint: undefined,
      })),
    );

    expect(resolveConfigsExtensionSpecs(["/a.toml", "/b.toml"])).toEqual([
      { path: "/a/ext/a.so", entrypoint: undefined },
      { path: "/b/sqlite_vec", entrypoint: undefined },
    ]);
    expect(vi.mocked(resolveEntries).mock.calls).toEqual([
      [[{ path: "ext/a.so" }], "/a"],
      [[{ path: "sqlite_vec" }], "/b"],
    ]);
  });

  it("skips a missing config but resolves the rest", () => {
    vi.mocked(loadExtensionEntries)
      .mockReturnValueOnce(null)
      .mockReturnValueOnce(loaded("/b", [{ path: "sqlite_vec" }]));
    vi.mocked(hasBareName).mockReturnValue(true);
    vi.mocked(resolveEntries).mockReturnValue([
      { path: "/nm/vec0.so", entrypoint: undefined },
    ]);

    expect(resolveConfigsExtensionSpecs(["/missing.toml", "/b.toml"])).toEqual([
      { path: "/nm/vec0.so", entrypoint: undefined },
    ]);
    expect(resolveEntries).toHaveBeenCalledTimes(1);
  });
});
