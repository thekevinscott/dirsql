import { describe, expect, it, vi } from "vitest";
import { loadNativeCore } from "./loadNativeCore.js";

describe("loadNativeCore", () => {
  describe("when the platform sub-package is installed", () => {
    it("loads via the `@dirsql/lib-<slug>` require and never hits the dev fallback", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec === "@dirsql/lib-linux-x64-gnu") return fakeCore;
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
          throw new Error("Cannot find module '@dirsql/lib-darwin-arm64'");
        }
        if (spec.endsWith("dirsql.node")) return fakeCore;
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

  describe("on a platform with no corresponding sub-package", () => {
    it("goes straight to the dev fallback without attempting a sub-package load", () => {
      const fakeCore = { DirSQL: vi.fn() };
      const requirer = vi.fn((spec: string) => {
        if (spec.endsWith("dirsql.node")) return fakeCore;
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
