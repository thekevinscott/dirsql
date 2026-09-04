import { existsSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type PackageResolver, packageDir } from "./package-dir.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
}));

function fakeResolver(over: Partial<PackageResolver> = {}): PackageResolver {
  return {
    resolve: vi.fn(() => {
      throw new Error("not resolvable");
    }),
    paths: vi.fn(() => []),
    ...over,
  };
}

function fakeStat(isDirectory: boolean) {
  return { isDirectory: () => isDirectory } as ReturnType<typeof statSync>;
}

describe("packageDir", () => {
  afterEach(() => vi.resetAllMocks());

  it("reads the package root off its package.json", () => {
    const resolver = fakeResolver({
      resolve: vi.fn(() => "/nm/sqlite-vec/package.json"),
    });
    expect(packageDir("sqlite-vec", resolver)).toBe("/nm/sqlite-vec");
    expect(resolver.resolve).toHaveBeenCalledWith("sqlite-vec/package.json");
    expect(resolver.paths).not.toHaveBeenCalled();
  });

  it("scans the candidate node_modules dirs when package.json is hidden", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(statSync).mockReturnValue(fakeStat(true));
    const resolver = fakeResolver({ paths: vi.fn(() => ["/nm"]) });
    expect(packageDir("sqlite-vec", resolver)).toBe("/nm/sqlite-vec");
    expect(resolver.paths).toHaveBeenCalledWith("sqlite-vec");
    expect(existsSync).toHaveBeenCalledWith("/nm/sqlite-vec");
  });

  it("skips a candidate that does not exist", () => {
    vi.mocked(existsSync).mockReturnValueOnce(false).mockReturnValue(true);
    vi.mocked(statSync).mockReturnValue(fakeStat(true));
    const resolver = fakeResolver({ paths: vi.fn(() => ["/a", "/b"]) });
    expect(packageDir("x", resolver)).toBe("/b/x");
  });

  it("skips a candidate that exists but is not a directory", () => {
    vi.mocked(existsSync).mockReturnValue(true);
    vi.mocked(statSync)
      .mockReturnValueOnce(fakeStat(false))
      .mockReturnValue(fakeStat(true));
    const resolver = fakeResolver({ paths: vi.fn(() => ["/a", "/b"]) });
    expect(packageDir("x", resolver)).toBe("/b/x");
  });

  it("errors when no candidate dir holds the package", () => {
    vi.mocked(existsSync).mockReturnValue(false);
    const resolver = fakeResolver({ paths: vi.fn(() => ["/a"]) });
    expect(() => packageDir("nope", resolver)).toThrow(
      "could not resolve extension package 'nope': not installed",
    );
  });

  it("treats null candidate dirs as no candidates", () => {
    // `require.resolve.paths` returns null for a builtin specifier.
    const resolver = fakeResolver({ paths: vi.fn(() => null) });
    expect(() => packageDir("nope", resolver)).toThrow(/not installed/);
    expect(existsSync).not.toHaveBeenCalled();
  });
});
