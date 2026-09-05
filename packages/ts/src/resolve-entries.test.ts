import { describe, expect, it, vi } from "vitest";
import { resolveEntries } from "./resolve-entries.js";
import { resolveExtensionPath } from "./resolve-extension.js";

vi.mock("./resolve-extension.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-extension.js")>(
    "./resolve-extension.js",
  )),
  resolveExtensionPath: vi.fn((path: string) => `R:${path}`),
}));

describe("resolveEntries", () => {
  it("resolves every path against the config's own directory", () => {
    expect(
      resolveEntries(
        [{ path: "sqlite_vec" }, { path: "ext/local.so" }],
        "/cfg",
      ),
    ).toEqual([
      { path: "R:sqlite_vec", entrypoint: undefined },
      { path: "R:ext/local.so", entrypoint: undefined },
    ]);
    expect(vi.mocked(resolveExtensionPath).mock.calls).toEqual([
      ["sqlite_vec", "/cfg", true],
      ["ext/local.so", "/cfg", true],
    ]);
  });

  it("keeps a string entrypoint", () => {
    expect(
      resolveEntries([{ path: "v", entrypoint: "sqlite3_vec_init" }], "/cfg"),
    ).toEqual([{ path: "R:v", entrypoint: "sqlite3_vec_init" }]);
  });

  it("normalizes a non-string entrypoint to undefined", () => {
    expect(resolveEntries([{ path: "v", entrypoint: 42 }], "/cfg")).toEqual([
      { path: "R:v", entrypoint: undefined },
    ]);
  });

  it("returns an empty list for no entries", () => {
    expect(resolveEntries([], "/cfg")).toEqual([]);
  });
});
