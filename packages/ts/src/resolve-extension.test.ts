// Unit tests for `resolveExtensionPath` (#299).
//
// The effectful collaborators are mocked: `node:fs` (existsSync / statSync /
// readdirSync) via `vi.mock`, and `require.resolve` via an injected
// `PackageResolver` fake (mirroring `load-native-core`'s injected requirer).
// `process.platform` is pinned per-test so the loadable glob is deterministic.

import { existsSync, readdirSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  type PackageResolver,
  isBareName,
  resolveExtensionPath,
} from "./resolve-extension.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
  readdirSync: vi.fn(),
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
    });
  });

  describe("bare-name package resolution", () => {
    it("globs the platform loadable inside the package dir (via package.json)", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(readdirSync).mockReturnValue([
        "README.md",
        "vec0.so",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver({
        resolve: vi.fn(() => "/nm/sqlite-vec/package.json"),
      });
      try {
        expect(resolveExtensionPath("sqlite-vec", "/cfg", true, resolver)).toBe(
          "/nm/sqlite-vec/vec0.so",
        );
        expect(resolver.resolve).toHaveBeenCalledWith(
          "sqlite-vec/package.json",
        );
      } finally {
        restore();
      }
    });

    it("falls back to require.resolve.paths when package.json is hidden", () => {
      const restore = pinPlatform("linux");
      // First existsSync (shadow probe) false; then the node_modules candidate
      // exists and is a directory.
      vi.mocked(existsSync).mockReturnValueOnce(false).mockReturnValue(true);
      vi.mocked(statSync).mockReturnValue({
        isFile: () => false,
        isDirectory: () => true,
      } as ReturnType<typeof statSync>);
      vi.mocked(readdirSync).mockReturnValue([
        "vec0.so",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver({ paths: vi.fn(() => ["/nm"]) });
      try {
        expect(resolveExtensionPath("sqlite-vec", "/cfg", true, resolver)).toBe(
          "/nm/sqlite-vec/vec0.so",
        );
      } finally {
        restore();
      }
    });

    it("globs dylib on macOS", () => {
      const restore = pinPlatform("darwin");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(readdirSync).mockReturnValue([
        "y.dylib",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver({
        resolve: vi.fn(() => "/p/x/package.json"),
      });
      try {
        expect(resolveExtensionPath("x", "/c", true, resolver)).toBe(
          "/p/x/y.dylib",
        );
      } finally {
        restore();
      }
    });

    it("globs dll on windows", () => {
      const restore = pinPlatform("win32");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(readdirSync).mockReturnValue(["y.dll"] as unknown as ReturnType<
        typeof readdirSync
      >);
      const resolver = fakeResolver({
        resolve: vi.fn(() => "/p/x/package.json"),
      });
      try {
        expect(resolveExtensionPath("x", "/c", true, resolver)).toBe(
          "/p/x/y.dll",
        );
      } finally {
        restore();
      }
    });

    it("errors when the package is not installed (null resolve paths)", () => {
      vi.mocked(existsSync).mockReturnValue(false);
      // `require.resolve.paths` can return null; the resolver must treat that
      // as "no candidate dirs" rather than throwing.
      const resolver = fakeResolver({ paths: vi.fn(() => null) });
      expect(() => resolveExtensionPath("nope", "/c", true, resolver)).toThrow(
        /not installed/,
      );
    });

    it("uses the default require-based resolver when none is injected", () => {
      // No resolver argument -> the real `createRequire`-backed resolver runs.
      // A package that isn't installed makes `require.resolve` throw and
      // `require.resolve.paths` yield only real node_modules dirs (none holding
      // it), so resolution falls through to the "not installed" error -- with
      // fs mocked, no real disk is touched.
      vi.mocked(existsSync).mockReturnValue(false);
      expect(() =>
        resolveExtensionPath("dirsql-nonexistent-pkg-xyz", "/c", true),
      ).toThrow(/not installed/);
    });

    it("errors when no loadable file is found", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(readdirSync).mockReturnValue([
        "README.md",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver({
        resolve: vi.fn(() => "/p/x/package.json"),
      });
      try {
        expect(() => resolveExtensionPath("x", "/c", true, resolver)).toThrow(
          /no loadable extension file/,
        );
      } finally {
        restore();
      }
    });

    it("errors when multiple loadable files are found", () => {
      const restore = pinPlatform("linux");
      vi.mocked(existsSync).mockReturnValue(false);
      vi.mocked(readdirSync).mockReturnValue([
        "a.so",
        "b.so",
      ] as unknown as ReturnType<typeof readdirSync>);
      const resolver = fakeResolver({
        resolve: vi.fn(() => "/p/x/package.json"),
      });
      try {
        expect(() => resolveExtensionPath("x", "/c", true, resolver)).toThrow(
          /multiple loadable extension files/,
        );
      } finally {
        restore();
      }
    });
  });
});
