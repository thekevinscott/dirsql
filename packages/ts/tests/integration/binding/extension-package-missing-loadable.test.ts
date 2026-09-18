// A real installed package containing no loadable file at all: the SDK must
// report which suffix patterns it looked for. The wording comes from the
// core's `SelectError`, so the patterns render as globs (`*.so`), not bare
// suffixes. Real `node_modules` layout, no mocks.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const tsNodeModules = resolve(
  import.meta.dirname,
  "..",
  "..",
  "..",
  "node_modules",
);

describe("DirSQL extension package with no loadable file", () => {
  // Must stay unique across test files: suites run in parallel workers and
  // share this `node_modules`.
  const pkgName = "dirsql-testext-empty-pkg";
  const pkgDir = join(tsNodeModules, pkgName);
  let tmp: string;

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "dirsql-ext-empty-"));
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({ name: pkgName, version: "0.0.0" }),
    );
    writeFileSync(join(pkgDir, "README.md"), "no loadable here\n");
  });

  afterEach(() => {
    rmSync(pkgDir, { recursive: true, force: true });
    rmSync(tmp, { recursive: true, force: true });
  });

  it("names the searched suffixes as glob patterns", async () => {
    const db = new DirSQL({ root: tmp, extensions: [{ path: pkgName }] });
    await expect(db.ready).rejects.toThrow(
      new RegExp(
        `no loadable extension file \\(\\*\\.[a-z]+( / \\*\\.[a-z]+)*\\) found in package '${pkgName}' \\(searched ${pkgDir}\\)`,
      ),
    );
  });
});
