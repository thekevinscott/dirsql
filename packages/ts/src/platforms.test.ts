import { describe, expect, it } from "vitest";
import {
  PLATFORMS,
  type Platform,
  libTriples,
  librarySlug,
} from "./platforms.js";

describe("PLATFORMS", () => {
  describe("shape invariants", () => {
    it("lists all five target triples", () => {
      const triples = PLATFORMS.map((p) => p.triple).sort();
      expect(triples).toEqual([
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
      ]);
    });

    it("uses the `@dirsql/lib-` npm scope for every napi sub-package", () => {
      for (const p of PLATFORMS) {
        expect(p.libName).toMatch(/^@dirsql\/lib-/);
      }
    });

    it("gives each platform a distinct libName", () => {
      const libs = PLATFORMS.map((p) => p.libName);
      expect(new Set(libs).size).toBe(libs.length);
    });

    it("declares `libc` only on Linux targets", () => {
      for (const p of PLATFORMS) {
        if (p.os.includes("linux")) {
          expect(p.libc).toEqual(["glibc"]);
        } else {
          expect(p.libc).toBeUndefined();
        }
      }
    });

    it("declares no standalone-CLI package name", () => {
      // The `@dirsql/cli-*` family is gone (#739): the addon carries the
      // CLI, so a second per-platform package would ship the core twice.
      for (const p of PLATFORMS) {
        expect(p).not.toHaveProperty("name");
      }
    });
  });

  describe("libTriples()", () => {
    it("maps every `${platform}-${arch}` key to its lib-* sub-package", () => {
      const map = libTriples();
      expect(Object.keys(map).length).toBe(PLATFORMS.length);
      for (const p of PLATFORMS) {
        expect(map[`${p.nodePlatform}-${p.nodeArch}`]).toBe(p.libName);
      }
    });
  });

  describe("librarySlug()", () => {
    it("returns the slug after the `@dirsql/lib-` prefix for every platform", () => {
      for (const p of PLATFORMS) {
        expect(librarySlug(p)).toBe(p.libName.slice("@dirsql/lib-".length));
      }
      expect(librarySlug(PLATFORMS[0] as Platform)).toBe("linux-x64-gnu");
    });

    it("throws when libName does not start with the `@dirsql/lib-` prefix", () => {
      const bad = {
        ...PLATFORMS[0],
        libName: "@dirsql/cli-linux-x64-gnu",
      } as Platform;
      expect(() => librarySlug(bad)).toThrow(
        "libName @dirsql/cli-linux-x64-gnu missing @dirsql/lib- prefix",
      );
    });
  });
});
