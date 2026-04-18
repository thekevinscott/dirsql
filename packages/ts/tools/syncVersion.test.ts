import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defaultPkgPath, syncVersion } from "./syncVersion.js";

let tmpPkg: string;
let pkgPath: string;

beforeEach(() => {
  tmpPkg = mkdtempSync(join(tmpdir(), "syncVersion-"));
  pkgPath = join(tmpPkg, "package.json");
});

afterEach(() => {
  rmSync(tmpPkg, { recursive: true, force: true });
  vi.restoreAllMocks();
});

function writePkgJson(contents: Record<string, unknown>) {
  writeFileSync(pkgPath, `${JSON.stringify(contents, null, 2)}\n`);
}

function readPkg(): {
  version: string;
  optionalDependencies?: Record<string, string>;
} {
  return JSON.parse(readFileSync(pkgPath, "utf8"));
}

describe("defaultPkgPath", () => {
  it("resolves to the sibling package.json in packages/ts/", () => {
    const p = defaultPkgPath();
    expect(p).toMatch(/packages\/ts\/package\.json$/);
  });
});

describe("syncVersion", () => {
  describe("with a plain version tag", () => {
    it("writes the version into package.json and strips a leading `v`", () => {
      writePkgJson({
        name: "dirsql",
        version: "0.0.0",
        optionalDependencies: { "@dirsql/cli-linux-x64-gnu": "0.0.0" },
      });
      syncVersion("v0.5.0", pkgPath);
      const pkg = readPkg();
      expect(pkg.version).toBe("0.5.0");
      expect(pkg.optionalDependencies?.["@dirsql/cli-linux-x64-gnu"]).toBe(
        "0.5.0",
      );
    });
  });

  describe("with many optionalDependencies", () => {
    it("rewrites every entry to match", () => {
      writePkgJson({
        name: "dirsql",
        version: "0.0.0",
        optionalDependencies: {
          "@dirsql/cli-linux-x64-gnu": "0.0.0",
          "@dirsql/cli-darwin-arm64": "0.0.0",
          "@dirsql/cli-win32-x64-msvc": "0.0.0",
        },
      });
      syncVersion("1.2.3", pkgPath);
      const pkg = readPkg();
      for (const v of Object.values(pkg.optionalDependencies ?? {})) {
        expect(v).toBe("1.2.3");
      }
    });
  });

  describe("without any optionalDependencies", () => {
    it("updates the version alone", () => {
      writePkgJson({ name: "dirsql", version: "0.0.0" });
      syncVersion("2.0.0", pkgPath);
      expect(readPkg().version).toBe("2.0.0");
    });
  });

  describe("with a non-v-prefixed tag", () => {
    it("keeps the literal version as-is", () => {
      writePkgJson({ name: "dirsql", version: "0.0.0" });
      syncVersion("1.0.0-rc.1", pkgPath);
      expect(readPkg().version).toBe("1.0.0-rc.1");
    });
  });

  describe("with an empty tag", () => {
    it("writes a `syncVersion:` prefixed stderr line and exits 1", () => {
      writePkgJson({ name: "dirsql", version: "0.0.0" });
      const stderr = vi.spyOn(process.stderr, "write").mockReturnValue(true);
      const exit = vi.spyOn(process, "exit").mockImplementation(((
        code?: string | number | null,
      ) => {
        throw new Error(`exit:${code}`);
      }) as typeof process.exit);

      expect(() => syncVersion("", pkgPath)).toThrow(/exit:1/);
      expect(stderr).toHaveBeenCalledWith(
        expect.stringContaining("syncVersion: missing version argument"),
      );
      expect(exit).toHaveBeenCalledWith(1);
    });
  });
});
