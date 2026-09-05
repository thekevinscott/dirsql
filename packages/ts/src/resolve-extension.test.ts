import { existsSync, statSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { defaultResolver } from "./default-resolver.js";
import { isBareName } from "./is-bare-name.js";
import type { PackageResolver } from "./package-dir.js";
import { resolveExtensionPath } from "./resolve-extension.js";
import { resolvePackage } from "./resolve-package.js";

vi.mock("node:fs", async () => ({
  ...(await vi.importActual<typeof import("node:fs")>("node:fs")),
  existsSync: vi.fn(),
  statSync: vi.fn(),
}));

vi.mock("./default-resolver.js", async () => ({
  ...(await vi.importActual<typeof import("./default-resolver.js")>(
    "./default-resolver.js",
  )),
  defaultResolver: vi.fn(),
}));

vi.mock("./is-bare-name.js", async () => ({
  ...(await vi.importActual<typeof import("./is-bare-name.js")>(
    "./is-bare-name.js",
  )),
  isBareName: vi.fn(),
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

describe("resolveExtensionPath", () => {
  afterEach(() => vi.resetAllMocks());

  describe("path-looking values", () => {
    it("makes a relative path absolute when resolveRelative", () => {
      vi.mocked(isBareName).mockReturnValue(false);
      expect(resolveExtensionPath("ext/a.so", "/cfg", true)).toBe(
        "/cfg/ext/a.so",
      );
      expect(isBareName).toHaveBeenCalledWith("ext/a.so");
    });

    it("preserves an absolute path when resolveRelative", () => {
      vi.mocked(isBareName).mockReturnValue(false);
      expect(resolveExtensionPath("/abs/a.so", "/cfg", true)).toBe("/abs/a.so");
    });

    it("returns a path verbatim when not resolveRelative", () => {
      vi.mocked(isBareName).mockReturnValue(false);
      expect(resolveExtensionPath("rel/a.so", "/cfg", false)).toBe("rel/a.so");
    });
  });

  describe("bare-name shadowing", () => {
    it("uses a same-named local file when present", () => {
      vi.mocked(isBareName).mockReturnValue(true);
      vi.mocked(existsSync).mockReturnValue(true);
      vi.mocked(statSync).mockReturnValue({ isFile: () => true } as ReturnType<
        typeof statSync
      >);
      expect(resolveExtensionPath("vec", "/cfg", true)).toBe("/cfg/vec");
      expect(resolvePackage).not.toHaveBeenCalled();
    });

    it("falls through when the same-named local entry is not a file", () => {
      vi.mocked(isBareName).mockReturnValue(true);
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
      vi.mocked(isBareName).mockReturnValue(true);
      vi.mocked(resolvePackage).mockReturnValue("/nm/pkg/vec0.so");
      vi.mocked(existsSync).mockReturnValue(false);
      const resolver = fakeResolver();
      expect(resolveExtensionPath("sqlite-vec", "/cfg", true, resolver)).toBe(
        "/nm/pkg/vec0.so",
      );
      expect(resolvePackage).toHaveBeenCalledWith("sqlite-vec", resolver);
      expect(defaultResolver).not.toHaveBeenCalled();
    });

    it("hands defaultResolver() to resolvePackage when none is injected", () => {
      const fallback = fakeResolver();
      vi.mocked(isBareName).mockReturnValue(true);
      vi.mocked(defaultResolver).mockReturnValue(fallback);
      vi.mocked(resolvePackage).mockReturnValue("/nm/pkg/vec0.so");
      vi.mocked(existsSync).mockReturnValue(false);
      expect(resolveExtensionPath("x", "/c", true)).toBe("/nm/pkg/vec0.so");
      expect(resolvePackage).toHaveBeenCalledWith("x", fallback);
    });
  });
});
