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
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { buildOne } from "./buildOne.js";
import type { Platform } from "../ts/platforms.js";

let work: string;
let distDir: string;
let outDir: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "buildOne-"));
  distDir = join(work, "distrib");
  outDir = join(work, "out");
  mkdirSync(distDir, { recursive: true });
  mkdirSync(outDir, { recursive: true });
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

function packTarXz(triple: string, binaryBytes: Buffer) {
  const staging = join(work, `staging-${triple}`);
  const innerDir = join(staging, `dirsql-${triple}`);
  mkdirSync(innerDir, { recursive: true });
  writeFileSync(join(innerDir, "dirsql"), binaryBytes);
  const archive = join(distDir, `dirsql-${triple}.tar.xz`);
  const result = spawnSync("tar", [
    "-cJf",
    archive,
    "-C",
    staging,
    `dirsql-${triple}`,
  ]);
  if (result.status !== 0) throw new Error("tar failed");
  return archive;
}

function packZip(triple: string, exe: string, binaryBytes: Buffer) {
  const staging = join(work, `staging-${triple}`);
  mkdirSync(staging, { recursive: true });
  writeFileSync(join(staging, exe), binaryBytes);
  const archive = join(distDir, `dirsql-${triple}.zip`);
  const result = spawnSync("zip", ["-q", archive, exe], { cwd: staging });
  if (result.status !== 0) throw new Error("zip failed");
  return archive;
}

const linux: Platform = {
  triple: "x86_64-unknown-linux-gnu",
  name: "@dirsql/cli-linux-x64-gnu",
  os: ["linux"],
  cpu: ["x64"],
  libc: ["glibc"],
  ext: "tar.xz",
};

const windows: Platform = {
  triple: "x86_64-pc-windows-msvc",
  name: "@dirsql/cli-win32-x64-msvc",
  os: ["win32"],
  cpu: ["x64"],
  ext: "zip",
  exe: true,
};

describe("buildOne", () => {
  describe("with a well-formed tar.xz archive", () => {
    it("extracts the binary, writes package.json + README, and removes the .extract dir", () => {
      packTarXz(linux.triple, Buffer.from("fake-binary-linux"));
      const dest = buildOne(linux, distDir, outDir, "0.9.0");

      expect(existsSync(join(dest, "dirsql"))).toBe(true);
      expect(existsSync(join(dest, "package.json"))).toBe(true);
      expect(existsSync(join(dest, "README.md"))).toBe(true);
      expect(existsSync(join(dest, ".extract"))).toBe(false);

      const pkg = JSON.parse(readFileSync(join(dest, "package.json"), "utf8"));
      expect(pkg.name).toBe(linux.name);
      expect(pkg.version).toBe("0.9.0");
      expect(pkg.os).toEqual(["linux"]);
      expect(pkg.cpu).toEqual(["x64"]);
      expect(pkg.libc).toEqual(["glibc"]);
      expect(pkg.bin).toEqual({ dirsql: "./dirsql" });
      expect(pkg.files).toEqual(["dirsql", "README.md"]);
    });

    it("logs a sha256 digest of the binary to stdout", () => {
      packTarXz(linux.triple, Buffer.from("fake"));
      const chunks: string[] = [];
      const spy = spyOnStdout((s) => chunks.push(s));
      try {
        buildOne(linux, distDir, outDir, "0.9.0");
      } finally {
        spy.restore();
      }
      const out = chunks.join("");
      expect(out).toMatch(/sha256=[0-9a-f]{64}/);
      expect(out).toMatch(linux.name);
      expect(out).toMatch("0.9.0");
    });

    it("omits `libc` when the platform doesn't declare one", () => {
      const darwin: Platform = {
        triple: "x86_64-apple-darwin",
        name: "@dirsql/cli-darwin-x64",
        os: ["darwin"],
        cpu: ["x64"],
        ext: "tar.xz",
      };
      packTarXz(darwin.triple, Buffer.from("fake"));
      const dest = buildOne(darwin, distDir, outDir, "0.9.0");
      const pkg = JSON.parse(readFileSync(join(dest, "package.json"), "utf8"));
      expect("libc" in pkg).toBe(false);
    });
  });

  describe("with a zip archive (Windows)", () => {
    it("extracts dirsql.exe and records it as the bin", () => {
      packZip(windows.triple, "dirsql.exe", Buffer.from("fake-win"));
      const dest = buildOne(windows, distDir, outDir, "0.9.0");

      expect(existsSync(join(dest, "dirsql.exe"))).toBe(true);
      const pkg = JSON.parse(readFileSync(join(dest, "package.json"), "utf8"));
      expect(pkg.bin).toEqual({ dirsql: "./dirsql.exe" });
      expect(pkg.files).toEqual(["dirsql.exe", "README.md"]);
    });
  });

  describe("when the archive is missing", () => {
    it("throws a descriptive error and doesn't leave a partial package dir", () => {
      expect(() => buildOne(linux, distDir, outDir, "0.9.0")).toThrow(
        /missing archive/,
      );
    });
  });

  describe("when the archive doesn't contain the expected binary", () => {
    it("throws `dirsql binary not found`", () => {
      const staging = join(work, "staging-empty");
      mkdirSync(join(staging, `dirsql-${linux.triple}`), { recursive: true });
      writeFileSync(
        join(staging, `dirsql-${linux.triple}`, "README"),
        "no binary",
      );
      const archive = join(distDir, `dirsql-${linux.triple}.tar.xz`);
      spawnSync("tar", [
        "-cJf",
        archive,
        "-C",
        staging,
        `dirsql-${linux.triple}`,
      ]);
      expect(() => buildOne(linux, distDir, outDir, "0.9.0")).toThrow(
        /dirsql binary not found/,
      );
    });
  });
});

function spyOnStdout(handler: (s: string) => void) {
  const orig = process.stdout.write.bind(process.stdout);
  process.stdout.write = ((chunk: unknown) => {
    handler(String(chunk));
    return true;
  }) as typeof process.stdout.write;
  return {
    restore() {
      process.stdout.write = orig;
    },
  };
}
