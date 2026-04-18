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
import { extract } from "./extract.js";

let work: string;

beforeEach(() => {
  work = mkdtempSync(join(tmpdir(), "extract-"));
});

afterEach(() => {
  rmSync(work, { recursive: true, force: true });
});

function createTarXz(): string {
  const staging = join(work, "staging");
  mkdirSync(staging, { recursive: true });
  writeFileSync(join(staging, "hello.txt"), "hi");
  const archive = join(work, "sample.tar.xz");
  const tar = spawnSync("tar", ["-cJf", archive, "-C", staging, "hello.txt"]);
  if (tar.status !== 0) throw new Error("tar failed during test setup");
  return archive;
}

describe("extract", () => {
  describe("with a tar.xz archive", () => {
    it("unpacks the archive into the destination directory", () => {
      const archive = createTarXz();
      const dest = join(work, "out");
      extract(archive, dest, "tar.xz");
      expect(existsSync(join(dest, "hello.txt"))).toBe(true);
      expect(readFileSync(join(dest, "hello.txt"), "utf8")).toBe("hi");
    });
  });

  describe("when the archive is missing", () => {
    it("throws a descriptive error", () => {
      expect(() =>
        extract(join(work, "does-not-exist.tar.xz"), join(work, "out"), "tar.xz"),
      ).toThrow(/extraction failed/);
    });
  });
});
