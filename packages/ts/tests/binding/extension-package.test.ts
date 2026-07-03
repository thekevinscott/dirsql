// Binding tier: resolve a SQLite extension by bare package name (#299).
//
// Lays a real compiled loadable inside a real installed-package directory under
// `node_modules`, points `DirSQL` at it by **bare package name**, and asserts
// the SDK resolves the actual on-disk file via `require.resolve`, loads it, and
// the function it registers is callable. Real layout, no mocks -- the shape
// mirrors the Python sibling (#298) and the Rust end-to-end test: resolve ->
// load -> callable.
//
// The loadable is the repo's `tests/fixtures/testext` cdylib (registers
// `dirsql_testext_answer() -> 42`), built on the fly with cargo.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const repoRoot = resolve(import.meta.dirname, "..", "..", "..", "..");
const tsNodeModules = resolve(import.meta.dirname, "..", "..", "node_modules");
const fixtureManifest = join(
  repoRoot,
  "packages/rust/tests/fixtures/testext/Cargo.toml",
);

function buildFixtureExtension(targetDir: string): string {
  const stdout = execFileSync(
    process.env.CARGO ?? "cargo",
    [
      "build",
      "--manifest-path",
      fixtureManifest,
      "--target-dir",
      targetDir,
      "--message-format=json",
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  let artifact: string | undefined;
  for (const line of stdout.split("\n")) {
    if (!line) {
      continue;
    }
    let msg: { reason?: string; filenames?: string[] };
    try {
      msg = JSON.parse(line);
    } catch {
      continue;
    }
    if (msg.reason === "compiler-artifact" && msg.filenames) {
      for (const f of msg.filenames) {
        if (f.endsWith(".so") || f.endsWith(".dylib") || f.endsWith(".dll")) {
          artifact = f;
        }
      }
    }
  }
  if (!artifact) {
    throw new Error("no cdylib artifact in cargo build output");
  }
  return artifact;
}

describe("DirSQL extension by package name (#299)", () => {
  // Must stay unique across test files: suites run in parallel workers and
  // share this `node_modules`, so a reused name races on setup/cleanup (#349).
  const pkgName = "dirsql-testext-pkg";
  const pkgDir = join(tsNodeModules, pkgName);
  let tmp: string;

  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "dirsql-ext-pkg-"));
    const so = buildFixtureExtension(join(tmp, "target"));
    // A real installed-package layout under the SDK's node_modules so
    // `require.resolve` finds it.
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({ name: pkgName, version: "0.0.0" }),
    );
    execFileSync("cp", [so, join(pkgDir, "testext.so")]);
  });

  afterEach(() => {
    rmSync(pkgDir, { recursive: true, force: true });
    rmSync(tmp, { recursive: true, force: true });
  });

  it("resolves, loads, and calls an extension referenced by package name", async () => {
    const db = new DirSQL({
      root: tmp,
      extensions: [{ path: pkgName, entrypoint: "sqlite3_extension_init" }],
    });
    await db.ready;
    expect(await db.query("SELECT dirsql_testext_answer() AS a")).toEqual([
      { a: 42 },
    ]);
  });
});
