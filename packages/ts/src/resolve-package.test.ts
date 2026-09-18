import { readdirSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getCore } from "./core.js";
import { type PackageResolver, packageDir } from "./package-dir.js";
import { resolvePackage } from "./resolve-package.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  readdirSync: vi.fn(),
}));
vi.mock("./core.js");
vi.mock("./package-dir.js", async () => ({
  ...(await vi.importActual<typeof import("./package-dir.js")>(
    "./package-dir.js",
  )),
  packageDir: vi.fn(),
}));

const selectLoadable =
  vi.fn<(name: string, dirs: string[], candidates: string[]) => string>();

function fakeResolver(): PackageResolver {
  return { resolve: vi.fn(), paths: vi.fn(() => []) };
}

beforeEach(() => {
  vi.mocked(getCore).mockReturnValue({ selectLoadable } as never);
});

afterEach(() => vi.resetAllMocks());

describe("resolvePackage", () => {
  it("hands the core every file under the package dir", () => {
    vi.mocked(packageDir).mockReturnValue("/nm/sqlite-vec");
    vi.mocked(readdirSync).mockReturnValue([
      "README.md",
      "dist/vec0.so",
    ] as unknown as ReturnType<typeof readdirSync>);
    selectLoadable.mockReturnValue("/nm/sqlite-vec/dist/vec0.so");
    const resolver = fakeResolver();

    expect(resolvePackage("sqlite-vec", resolver)).toBe(
      "/nm/sqlite-vec/dist/vec0.so",
    );
    expect(packageDir).toHaveBeenCalledWith("sqlite-vec", resolver);
    expect(readdirSync).toHaveBeenCalledWith("/nm/sqlite-vec", {
      recursive: true,
    });
    expect(selectLoadable).toHaveBeenCalledWith(
      "sqlite-vec",
      ["/nm/sqlite-vec"],
      ["/nm/sqlite-vec/README.md", "/nm/sqlite-vec/dist/vec0.so"],
    );
  });

  it("propagates the core's selection error", () => {
    vi.mocked(packageDir).mockReturnValue("/p/x");
    vi.mocked(readdirSync).mockReturnValue(
      [] as unknown as ReturnType<typeof readdirSync>,
    );
    selectLoadable.mockImplementation(() => {
      throw new Error("no loadable extension file (*.so) found in package 'x'");
    });

    expect(() => resolvePackage("x", fakeResolver())).toThrow(
      "no loadable extension file (*.so) found in package 'x'",
    );
  });
});
