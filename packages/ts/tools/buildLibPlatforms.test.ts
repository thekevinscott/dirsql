import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { buildLibPlatforms } from "./buildLibPlatforms.js";
import { PLATFORMS, librarySlug } from "../ts/platforms.js";

let work: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "buildLibPlatforms-"));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
  delete process.env.DIRSQL_NAPI_DIR;
  delete process.env.DIRSQL_LIB_OUT_DIR;
  delete process.env.DIRSQL_VERSION;
});

describe("buildLibPlatforms", () => {
  describe("with a .node artifact for every target", () => {
    it("emits a @dirsql/lib-* sub-package tree per platform", () => {
      const nodeDir = join(work, "napi");
      const outDir = join(work, "out");
      mkdirSync(nodeDir, { recursive: true });
      mkdirSync(outDir, { recursive: true });

      for (const p of PLATFORMS) {
        writeFileSync(
          join(nodeDir, `dirsql.${librarySlug(p)}.node`),
          Buffer.from(`fake-${p.triple}`),
        );
      }

      process.env.DIRSQL_NAPI_DIR = nodeDir;
      process.env.DIRSQL_LIB_OUT_DIR = outDir;
      process.env.DIRSQL_VERSION = "1.2.3";

      buildLibPlatforms();

      for (const p of PLATFORMS) {
        const pkgDir = join(outDir, p.libName.replace("/", "__"));
        expect(existsSync(join(pkgDir, "package.json"))).toBe(true);
        expect(existsSync(join(pkgDir, `dirsql.${librarySlug(p)}.node`))).toBe(
          true,
        );
      }
    });
  });
});
