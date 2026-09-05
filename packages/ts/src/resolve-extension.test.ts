import { existsSync, readdirSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type PackageResolver, packageDir } from "./package-dir.js";
import { isBareName, resolveExtensionPath } from "./resolve-extension.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
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

function fakeResolver(over: Partial<PackageResolver> = {}): PackageResolver {
  return {
    resolve: vi.fn(() => {
      throw new Error("not resolvable");
    }),
    paths: vi.fn(() => []),
    ...over,
  };
}

describe("isBareName", () => {
  it("treats a separatorful value as a path", () => {
    expect(isBareName("ext/vec0.so")).toBe(false);
    expect(isBareName("C:\\ext\\vec0.dll")).toBe(false);
  });

  it("treats a loadable suffix as a path", () => {
    expect(isBareName("vec0.so")).toBe(false);
    expect(isBareName("vec0.dylib")).toBe(false);
    expect(isBareName("vec0.dll")).toBe(false);
    expect(isBareName("vec0.node")).toBe(false);
  });

  it("treats a plain identifier as a bare name", () => {
    expect(isBareName("sqlite-vec")).toBe(true);
  });
});

describe("resolveExtensionPath", () => {
  afterEach(() => vi.resetAllMocks());

  describe("path-looking values", () => {
    it("makes a relative path absolute when resolveRelative", () => {
      expect(resolveExtensionPath("ext/a.so", "/cfg", true)).toBe(
        "/cfg/ext/a.so",
      );
    });

    it("preserves an absolute path when resolveRelative", () => {
      expect(resolveExtensionPath("/abs/a.so", "/cfg", true)).toBe("/abs/a.so");
    });

    it("returns a path verbatim when not resolveRelative", () => {
      expect(resolveExtensionPath("rel/a.so", "/cfg", false)).toBe("rel/a.so");
    });
  });

  describe("bare-name shadowing", () => {
    it("uses a same-named local file when present", () => {
      vi.mocked(existsSync).mockReturnValue(true);
      vi.mocked(statSync).mockReturnValue({ isFile: () => true } as ReturnType<
        typeof statSync
      >);
      expect(resolveExtensionPath("vec", "/cfg", true)).toBe("/cfg/vec");
      expect(packageDir).not.toHaveBeenCalled();
    });
  });

  describe("bare-name package resolution", () => {
    it("globs the platform loadable inside the package dir", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/nm/sqlite-vec");
      vi.mocked(readdirSync).mockReturnValue([
        "README.md",
        "vec0.so",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver();
      try {
        expect(resolveExtensionPath("sqlite-vec", "/cfg", true, resolver)).toBe(
          "/nm/sqlite-vec/vec0.so",
        );
        expect(packageDir).toHaveBeenCalledWith("sqlite-vec", resolver);
      } finally {
        restore();
      }
    });

    it("globs dylib on macOS", () => {
      const restore = pinPlatform("darwin");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/p/x");
      vi.mocked(readdirSync).mockReturnValue([
        "y.dylib",
      ] as unknown as ReturnType<typeof readdirSync>);
      try {
        expect(resolveExtensionPath("x", "/c", true, fakeResolver())).toBe(
          "/p/x/y.dylib",
        );
      } finally {
        restore();
      }
    });

    it("globs dll on windows", () => {
      const restore = pinPlatform("win32");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/p/x");
      vi.mocked(readdirSync).mockReturnValue(["y.dll"] as unknown as ReturnType<
        typeof readdirSync
      >);
      try {
        expect(resolveExtensionPath("x", "/c", true, fakeResolver())).toBe(
          "/p/x/y.dll",
        );
      } finally {
        restore();
      }
    });

    it("hands a require-backed resolver to packageDir when none is injected", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/p/x");
      vi.mocked(readdirSync).mockReturnValue(["y.so"] as unknown as ReturnType<
        typeof readdirSync
      >);
      try {
        expect(resolveExtensionPath("x", "/c", true)).toBe("/p/x/y.so");
        const resolver = vi.mocked(packageDir).mock.calls[0]?.[1];
        expect(resolver?.paths("vitest")).toEqual(expect.any(Array));
        expect(() => resolver?.resolve("dirsql-nonexistent-pkg-xyz")).toThrow(
          /Cannot find module/,
        );
      } finally {
        restore();
      }
    });

    it("errors when no loadable file is found", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/p/x");
      vi.mocked(readdirSync).mockReturnValue([
        "README.md",
      ] as unknown as ReturnType<typeof readdirSync>);
      try {
        expect(() =>
          resolveExtensionPath("x", "/c", true, fakeResolver()),
        ).toThrow(
          "no loadable extension file (.so / .node) found in package 'x' (searched /p/x)",
        );
      } finally {
        restore();
      }
    });

    it("errors when multiple loadable files are found", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(packageDir).mockReturnValue("/p/x");
      vi.mocked(readdirSync).mockReturnValue([
        "b.so",
        "a.so",
      ] as unknown as ReturnType<typeof readdirSync>);
      try {
        expect(() =>
          resolveExtensionPath("x", "/c", true, fakeResolver()),
        ).toThrow(
          "multiple loadable extension files found in package 'x': /p/x/a.so, /p/x/b.so; disambiguate with a literal path",
        );
      } finally {
        restore();
      }
    });
  });
});
