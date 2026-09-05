import { readdirSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type PackageResolver, packageDir } from "./package-dir.js";
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

function pinPlatform(value: NodeJS.Platform) {
  const original = process.platform;
  Object.defineProperty(process, "platform", { value, configurable: true });
  return () =>
    Object.defineProperty(process, "platform", {
      value: original,
      configurable: true,
    });
}

function fakeResolver(): PackageResolver {
  return {
    resolve: vi.fn(() => {
      throw new Error("not resolvable");
    }),
    paths: vi.fn(() => []),
  };
}

function stubDir(dir: string, entries: string[]) {
  vi.mocked(packageDir).mockReturnValue(dir);
  vi.mocked(readdirSync).mockReturnValue(
    entries as unknown as ReturnType<typeof readdirSync>,
  );
}

describe("resolvePackage", () => {
  afterEach(() => vi.resetAllMocks());

  it("globs the platform loadable inside the package dir", () => {
    const restore = pinPlatform("linux");
    stubDir("/nm/sqlite-vec", ["README.md", "vec0.so"]);
    const resolver = fakeResolver();
    try {
      expect(resolvePackage("sqlite-vec", resolver)).toBe(
        "/nm/sqlite-vec/vec0.so",
      );
      expect(packageDir).toHaveBeenCalledWith("sqlite-vec", resolver);
      expect(readdirSync).toHaveBeenCalledWith("/nm/sqlite-vec", {
        recursive: true,
      });
    } finally {
      restore();
    }
  });

  it("globs a .node file on linux", () => {
    const restore = pinPlatform("linux");
    stubDir("/p/x", ["README.md", "y.node"]);
    try {
      expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.node");
    } finally {
      restore();
    }
  });

  it("globs dylib on macOS", () => {
    const restore = pinPlatform("darwin");
    stubDir("/p/x", ["README.md", "y.dylib"]);
    try {
      expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.dylib");
    } finally {
      restore();
    }
  });

  it("globs a .node file on macOS", () => {
    const restore = pinPlatform("darwin");
    stubDir("/p/x", ["README.md", "y.node"]);
    try {
      expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.node");
    } finally {
      restore();
    }
  });

  it("globs dll on windows", () => {
    const restore = pinPlatform("win32");
    stubDir("/p/x", ["README.md", "y.dll"]);
    try {
      expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.dll");
    } finally {
      restore();
    }
  });

  it("globs a .node file on windows", () => {
    const restore = pinPlatform("win32");
    stubDir("/p/x", ["README.md", "y.node"]);
    try {
      expect(resolvePackage("x", fakeResolver())).toBe("/p/x/y.node");
    } finally {
      restore();
    }
  });

  it("ignores a loadable belonging to another platform", () => {
    const restore = pinPlatform("linux");
    stubDir("/p/x", ["y.dylib", "y.dll"]);
    try {
      expect(() => resolvePackage("x", fakeResolver())).toThrow(
        "no loadable extension file (.so / .node) found in package 'x' (searched /p/x)",
      );
    } finally {
      restore();
    }
  });

  it("errors when no loadable file is found", () => {
    const restore = pinPlatform("linux");
    stubDir("/p/x", ["README.md"]);
    try {
      expect(() => resolvePackage("x", fakeResolver())).toThrow(
        "no loadable extension file (.so / .node) found in package 'x' (searched /p/x)",
      );
    } finally {
      restore();
    }
  });

  it("names the macOS suffixes when nothing is found on macOS", () => {
    const restore = pinPlatform("darwin");
    stubDir("/p/x", ["README.md"]);
    try {
      expect(() => resolvePackage("x", fakeResolver())).toThrow(
        "no loadable extension file (.dylib / .node) found in package 'x' (searched /p/x)",
      );
    } finally {
      restore();
    }
  });

  it("names the windows suffixes when nothing is found on windows", () => {
    const restore = pinPlatform("win32");
    stubDir("/p/x", ["README.md"]);
    try {
      expect(() => resolvePackage("x", fakeResolver())).toThrow(
        "no loadable extension file (.dll / .node) found in package 'x' (searched /p/x)",
      );
    } finally {
      restore();
    }
  });

  it("errors when multiple loadable files are found, sorted", () => {
    const restore = pinPlatform("linux");
    stubDir("/p/x", ["b.so", "a.so"]);
    try {
      expect(() => resolvePackage("x", fakeResolver())).toThrow(
        "multiple loadable extension files found in package 'x': /p/x/a.so, /p/x/b.so; disambiguate with a literal path",
      );
    } finally {
      restore();
    }
  });
});
