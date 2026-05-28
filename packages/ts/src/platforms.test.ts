import { describe, expect, it } from "vitest";
import { PLATFORMS, libTriples, nodeTriples } from "./platforms.js";

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

    it("uses the `@dirsql/cli-` npm scope for every sub-package name", () => {
      for (const p of PLATFORMS) {
        expect(p.name).toMatch(/^@dirsql\/cli-/);
      }
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

    it("pairs tar.xz with unix targets and zip with windows", () => {
      for (const p of PLATFORMS) {
        if (p.os.includes("win32")) {
          expect(p.ext).toBe("zip");
          expect(p.exe).toBe(true);
        } else {
          expect(p.ext).toBe("tar.xz");
          expect(p.exe).toBeUndefined();
        }
      }
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

    it("has distinct names", () => {
      const names = PLATFORMS.map((p) => p.name);
      expect(new Set(names).size).toBe(names.length);
    });
  });

  describe("nodeTriples()", () => {
    it("maps every `${platform}-${arch}` key to its cli-* sub-package", () => {
      const map = nodeTriples();
      expect(Object.keys(map).length).toBe(PLATFORMS.length);
      for (const p of PLATFORMS) {
        expect(map[`${p.nodePlatform}-${p.nodeArch}`]).toBe(p.name);
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
});
