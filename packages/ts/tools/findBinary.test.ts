import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { findBinary } from "./findBinary.js";

let root: string;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "findBinary-"));
});

afterEach(() => {
  rmSync(root, { recursive: true, force: true });
});

describe("findBinary", () => {
  describe("when the target lives at the top level", () => {
    it("returns the direct path", () => {
      writeFileSync(join(root, "dirsql"), "");
      expect(findBinary(root, "dirsql")).toBe(join(root, "dirsql"));
    });
  });

  describe("when the target is nested", () => {
    it("walks into subdirectories and returns the full path", () => {
      mkdirSync(join(root, "dirsql-x86_64-unknown-linux-gnu"));
      writeFileSync(join(root, "dirsql-x86_64-unknown-linux-gnu", "dirsql"), "");
      expect(findBinary(root, "dirsql")).toBe(
        join(root, "dirsql-x86_64-unknown-linux-gnu", "dirsql"),
      );
    });
  });

  describe("when the target is absent", () => {
    it("returns null", () => {
      writeFileSync(join(root, "unrelated"), "");
      expect(findBinary(root, "dirsql")).toBeNull();
    });
  });
});
