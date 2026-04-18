// Builds a single `@dirsql/lib-<slug>` sub-package from a prebuilt
// `.node` artifact. See `buildLibOne` for details.

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
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { buildLibOne } from "./buildLibOne.js";
import type { Platform } from "../ts/platforms.js";

let work: string;
let nodeDir: string;
let outDir: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "buildLibOne-"));
  nodeDir = join(work, "node");
  outDir = join(work, "out");
  mkdirSync(nodeDir, { recursive: true });
  mkdirSync(outDir, { recursive: true });
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

const linux: Platform = {
  triple: "x86_64-unknown-linux-gnu",
  nodePlatform: "linux",
  nodeArch: "x64",
  name: "@dirsql/cli-linux-x64-gnu",
  libName: "@dirsql/lib-linux-x64-gnu",
  os: ["linux"],
  cpu: ["x64"],
  libc: ["glibc"],
  ext: "tar.xz",
};

const darwinArm: Platform = {
  triple: "aarch64-apple-darwin",
  nodePlatform: "darwin",
  nodeArch: "arm64",
  name: "@dirsql/cli-darwin-arm64",
  libName: "@dirsql/lib-darwin-arm64",
  os: ["darwin"],
  cpu: ["arm64"],
  ext: "tar.xz",
};

describe("buildLibOne", () => {
  describe("with a prebuilt `.node` file present", () => {
    it("emits a sub-package with the binary + package.json + README", () => {
      const binSrc = join(nodeDir, "dirsql.linux-x64-gnu.node");
      writeFileSync(binSrc, Buffer.from("fake-napi-linux"));

      const dest = buildLibOne(linux, nodeDir, outDir, "0.9.0");

      const binDest = join(dest, "dirsql.linux-x64-gnu.node");
      expect(existsSync(binDest)).toBe(true);
      expect(readFileSync(binDest).toString()).toBe("fake-napi-linux");

      const pkg = JSON.parse(readFileSync(join(dest, "package.json"), "utf8"));
      expect(pkg.name).toBe(linux.libName);
      expect(pkg.version).toBe("0.9.0");
      expect(pkg.main).toBe("dirsql.linux-x64-gnu.node");
      expect(pkg.os).toEqual(["linux"]);
      expect(pkg.cpu).toEqual(["x64"]);
      expect(pkg.libc).toEqual(["glibc"]);
      expect(pkg.files).toEqual([
        "dirsql.linux-x64-gnu.node",
        "README.md",
      ]);

      expect(existsSync(join(dest, "README.md"))).toBe(true);
    });

    it("omits `libc` on non-Linux platforms", () => {
      writeFileSync(
        join(nodeDir, "dirsql.darwin-arm64.node"),
        Buffer.from("fake-napi-darwin"),
      );
      const dest = buildLibOne(darwinArm, nodeDir, outDir, "0.9.0");
      const pkg = JSON.parse(readFileSync(join(dest, "package.json"), "utf8"));
      expect("libc" in pkg).toBe(false);
      expect(pkg.main).toBe("dirsql.darwin-arm64.node");
    });
  });

  describe("when the `.node` file is missing", () => {
    it("throws a descriptive error", () => {
      expect(() => buildLibOne(linux, nodeDir, outDir, "0.9.0")).toThrow(
        /missing napi artifact/,
      );
    });
  });
});
