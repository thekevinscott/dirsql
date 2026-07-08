import { describe, expect, it, vi } from "vitest";
import { loadNativeCore } from "./load-native-core.js";

describe("loadNativeCore", () => {
  describe("when the platform sub-package is installed", () => {
    it("loads via the `@dirsql/lib-<slug>` require and never hits the dev fallback", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec === "@dirsql/lib-linux-x64-gnu") {
          return fakeCore;
        }
        throw new Error(`unexpected require(${spec})`);
      });

      const core = loadNativeCore(
        "linux-x64",
        requirer as unknown as (s: string) => unknown,
        () => "/fake",
      );

      expect(core).toBe(fakeCore);
      expect(requirer).toHaveBeenCalledExactlyOnceWith(
        "@dirsql/lib-linux-x64-gnu",
      );
    });
  });

  describe("when the platform sub-package is missing", () => {
    it("falls back to `<pkgRoot>/dirsql.node`", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec === "@dirsql/lib-darwin-arm64") {
          const err = new Error(
            "Cannot find module '@dirsql/lib-darwin-arm64'",
          ) as NodeJS.ErrnoException;
          err.code = "MODULE_NOT_FOUND";
          throw err;
        }
        if (spec.endsWith("dirsql.node")) {
          return fakeCore;
        }
        throw new Error(`unexpected require(${spec})`);
      });

      const core = loadNativeCore(
        "darwin-arm64",
        requirer as unknown as (s: string) => unknown,
        () => "/pkg/ts",
      );

      expect(core).toBe(fakeCore);
      expect(requirer).toHaveBeenCalledTimes(2);
      expect(requirer).toHaveBeenLastCalledWith("/pkg/dirsql.node");
    });
  });

  describe("when the platform sub-package load fails for another reason", () => {
    it("propagates the original error instead of masking it with the dev fallback", () => {
      const glibcErr = new Error(
        "/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found",
      );
      const requirer = vi.fn((spec: string) => {
        if (spec === "@dirsql/lib-linux-x64-gnu") {
          throw glibcErr;
        }
        throw new Error(`unexpected require(${spec})`);
      });

      expect(() =>
        loadNativeCore(
          "linux-x64",
          requirer as unknown as (s: string) => unknown,
          () => "/pkg/ts",
        ),
      ).toThrow(glibcErr);
      expect(requirer).toHaveBeenCalledExactlyOnceWith(
        "@dirsql/lib-linux-x64-gnu",
      );
    });
  });

  describe("with the default dirname resolver", () => {
    it("resolves the dev fallback relative to this module's own directory", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec.endsWith("dirsql.node")) {
          return fakeCore;
        }
        throw new Error(`unexpected require(${spec})`);
      });

      // Omit the third arg so the default `() => import.meta.dirname` arrow
      // runs (the function the unit suite otherwise never invokes).
      const core = loadNativeCore(
        "freebsd-x64",
        requirer as unknown as (s: string) => unknown,
      );

      expect(core).toBe(fakeCore);
      expect(requirer).toHaveBeenCalledExactlyOnceWith(
        expect.stringMatching(/dirsql\.node$/),
      );
    });
  });

  describe("on a platform with no corresponding sub-package", () => {
    it("goes straight to the dev fallback without attempting a sub-package load", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec.endsWith("dirsql.node")) {
          return fakeCore;
        }
        throw new Error(`unexpected require(${spec})`);
      });

      const core = loadNativeCore(
        "freebsd-x64",
        requirer as unknown as (s: string) => unknown,
        () => "/pkg/ts",
      );

      expect(core).toBe(fakeCore);
      expect(requirer).toHaveBeenCalledExactlyOnceWith("/pkg/dirsql.node");
    });
  });
});
