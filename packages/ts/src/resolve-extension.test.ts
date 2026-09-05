import { existsSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PackageResolver } from "./package-dir.js";
import { isBareName, resolveExtensionPath } from "./resolve-extension.js";
import { resolvePackage } from "./resolve-package.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
}));

vi.mock("./resolve-package.js", async () => ({
  ...(await vi.importActual<typeof import("./resolve-package.js")>(
    "./resolve-package.js",
  )),
  resolvePackage: vi.fn(),
}));

function fakeResolver(): PackageResolver {
  return {
    resolve: vi.fn(() => {
      throw new Error("not resolvable");
    }),
    paths: vi.fn(() => []),
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
      expect(resolvePackage).not.toHaveBeenCalled();
    });

    it("falls through when the same-named local entry is not a file", () => {
      vi.mocked(resolvePackage).mockReturnValue("/nm/pkg/vec0.so");
      vi.mocked(existsSync).mockReturnValue(true);
      vi.mocked(statSync).mockReturnValue({ isFile: () => false } as ReturnType<
        typeof statSync
      >);
      expect(resolveExtensionPath("vec", "/cfg", true, fakeResolver())).toBe(
        "/nm/pkg/vec0.so",
      );
    });
  });

  describe("bare-name package resolution", () => {
    it("delegates to resolvePackage with the injected resolver", () => {
      vi.mocked(resolvePackage).mockReturnValue("/nm/pkg/vec0.so");
      vi.mocked(existsSync).mockReturnValue(false);
      const resolver = fakeResolver();
      expect(resolveExtensionPath("sqlite-vec", "/cfg", true, resolver)).toBe(
        "/nm/pkg/vec0.so",
      );
      expect(resolvePackage).toHaveBeenCalledWith("sqlite-vec", resolver);
    });

    it("hands a require-backed resolver to resolvePackage when none is injected", () => {
      vi.mocked(resolvePackage).mockReturnValue("/nm/pkg/vec0.so");
      vi.mocked(existsSync).mockReturnValue(false);
      expect(resolveExtensionPath("x", "/c", true)).toBe("/nm/pkg/vec0.so");
      const resolver = vi.mocked(resolvePackage).mock.calls[0]?.[1];
      expect(resolver?.paths("vitest")).toEqual(expect.any(Array));
      expect(() => resolver?.resolve("dirsql-nonexistent-pkg-xyz")).toThrow(
        /Cannot find module/,
      );
    });
  });
});
