import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildPlatforms } from "./buildPlatforms.js";
import { PLATFORMS } from "../ts/platforms.js";

let work: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "buildPlatforms-"));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
  vi.unstubAllEnvs();
});

function pack(triple: string, ext: "tar.xz" | "zip", binName: string) {
  const staging = join(work, `staging-${triple}`);
  const archive = join(work, "dist", `dirsql-${triple}.${ext}`);
  mkdirSync(join(work, "dist"), { recursive: true });
  if (ext === "tar.xz") {
    mkdirSync(join(staging, `dirsql-${triple}`), { recursive: true });
    writeFileSync(join(staging, `dirsql-${triple}`, binName), "fake");
    spawnSync("tar", [
      "-cJf",
      archive,
      "-C",
      staging,
      `dirsql-${triple}`,
    ]);
  } else {
    mkdirSync(staging, { recursive: true });
    writeFileSync(join(staging, binName), "fake");
    spawnSync("zip", ["-q", archive, binName], { cwd: staging });
  }
}

describe("buildPlatforms", () => {
  describe("with archives present for every target", () => {
    it("emits one per-platform package tree under DIRSQL_OUT_DIR", () => {
      for (const p of PLATFORMS) {
        pack(p.triple, p.ext, p.exe ? "dirsql.exe" : "dirsql");
      }
      vi.stubEnv("DIRSQL_DIST_DIR", join(work, "dist"));
      vi.stubEnv("DIRSQL_OUT_DIR", join(work, "out"));
      vi.stubEnv("DIRSQL_VERSION", "9.9.9");

      buildPlatforms();

      for (const p of PLATFORMS) {
        const dir = join(work, "out", p.name.replace("/", "__"));
        expect(existsSync(dir)).toBe(true);
        const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
        expect(pkg.name).toBe(p.name);
        expect(pkg.version).toBe("9.9.9");
      }
    });
  });

  describe("when a required archive is missing", () => {
    it("bubbles up the missing-archive error", () => {
      // Only pack one archive, then run: orchestrator must fail on a later target.
      pack(PLATFORMS[0].triple, PLATFORMS[0].ext, "dirsql");
      vi.stubEnv("DIRSQL_DIST_DIR", join(work, "dist"));
      vi.stubEnv("DIRSQL_OUT_DIR", join(work, "out"));
      vi.stubEnv("DIRSQL_VERSION", "9.9.9");

      expect(() => buildPlatforms()).toThrow(/missing archive/);
    });
  });

  describe("default environment", () => {
    it("falls back to packages/ts/package.json's version when DIRSQL_VERSION is unset", () => {
      for (const p of PLATFORMS) {
        pack(p.triple, p.ext, p.exe ? "dirsql.exe" : "dirsql");
      }
      vi.stubEnv("DIRSQL_DIST_DIR", join(work, "dist"));
      vi.stubEnv("DIRSQL_OUT_DIR", join(work, "out"));
      vi.stubEnv("DIRSQL_VERSION", "");

      buildPlatforms();

      const first = PLATFORMS[0];
      const pkg = JSON.parse(
        readFileSync(
          join(work, "out", first.name.replace("/", "__"), "package.json"),
          "utf8",
        ),
      );
      const repoPkg = JSON.parse(
        readFileSync(
          join(__dirname, "..", "package.json"),
          "utf8",
        ),
      );
      expect(pkg.version).toBe(repoPkg.version);
    });
  });
});
