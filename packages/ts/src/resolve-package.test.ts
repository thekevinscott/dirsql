import { readdirSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type PackageResolver, packageDir } from "./package-dir.js";
import { platformSuffixes } from "./platform-suffixes.js";
import { resolvePackage } from "./resolve-package.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  readdirSync: vi.fn(),
}));

vi.mock("./package-dir.js", async () => ({
  ...(await vi.importActual<typeof import("./package-dir.js")>(
    "./package-dir.js",
  )),
  packageDir: vi.fn(),
}));

vi.mock("./platform-suffixes.js", async () => ({
  ...(await vi.importActual<typeof import("./platform-suffixes.js")>(
    "./platform-suffixes.js",
  )),
  platformSuffixes: vi.fn(),
}));

function fakeResolver(): PackageResolver {
  return {
    resolve: vi.fn(() => {
      throw new Error("not resolvable");
    }),
    paths: vi.fn(() => []),
  };
}

function stubDir(dir: string, entries: string[], suffixes = [".so", ".node"]) {
  vi.mocked(packageDir).mockReturnValue(dir);
  vi.mocked(platformSuffixes).mockReturnValue(suffixes);
  vi.mocked(readdirSync).mockReturnValue(
    entries as unknown as ReturnType<typeof readdirSync>,
  );
}

describe("resolvePackage", () => {
  afterEach(() => vi.resetAllMocks());

  it("globs the platform loadable inside the package dir", () => {
    stubDir("/nm/sqlite-vec", ["README.md", "vec0.so"]);
    const resolver = fakeResolver();

    expect(resolvePackage("sqlite-vec", resolver)).toBe(
      "/nm/sqlite-vec/vec0.so",
    );
    expect(packageDir).toHaveBeenCalledWith("sqlite-vec", resolver);
    expect(readdirSync).toHaveBeenCalledWith("/nm/sqlite-vec", {
      recursive: true,
    });
  });

  it("matches any suffix the current platform globs for", () => {
    stubDir("/p/x", ["README.md", "y.node"]);
    expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.node");
  });

  it("takes its suffixes from platformSuffixes, not a hardcoded list", () => {
    stubDir("/p/x", ["y.so", "y.dylib"], [".dylib"]);
    expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.dylib");
  });

  it("ignores a loadable belonging to another platform", () => {
    stubDir("/p/x", ["y.dylib", "y.dll"]);
    expect(() => resolvePackage("x", fakeResolver())).toThrow(
      "no loadable extension file (.so / .node) found in package 'x' (searched /p/x)",
    );
  });

  it("errors when no loadable file is found", () => {
    stubDir("/p/x", ["README.md"]);
    expect(() => resolvePackage("x", fakeResolver())).toThrow(
      "no loadable extension file (.so / .node) found in package 'x' (searched /p/x)",
    );
  });

  it("names the globbed suffixes in the not-found message", () => {
    stubDir("/p/x", ["README.md"], [".dll", ".node"]);
    expect(() => resolvePackage("x", fakeResolver())).toThrow(
      "no loadable extension file (.dll / .node) found in package 'x' (searched /p/x)",
    );
  });

  it("errors when multiple loadable files are found, sorted", () => {
    stubDir("/p/x", ["b.so", "a.so"]);
    expect(() => resolvePackage("x", fakeResolver())).toThrow(
      "multiple loadable extension files found in package 'x': /p/x/a.so, /p/x/b.so; disambiguate with a literal path",
    );
  });
});
