// Lays a real compiled loadable inside a real installed-package directory
// under `node_modules`, declares it in a `.dirsql.toml` as a
// `[[dirsql.extension]]` entry whose `path` is a bare **package name**, and
// asserts that constructing `DirSQL` from that config resolves the on-disk
// file via `require.resolve`, loads it, and the function it registers is
// callable. Real layout, no mocks.
//
// The loadable is the repo's `tests/fixtures/testext` cdylib (registers
// `dirsql_testext_answer() -> 42`), built on the fly with cargo.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const repoRoot = resolve(import.meta.dirname, "..", "..", "..", "..", "..");
const tsNodeModules = resolve(
  import.meta.dirname,
  "..",
  "..",
  "..",
  "node_modules",
);
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

describe("DirSQL config-file extension by package name (#313)", () => {
  // Unique to this suite: `extension-package.test.ts` installs its own fake
  // package in the same shared `node_modules`, and vitest runs the two files
  // in parallel workers — a shared name lets one suite's cleanup delete the
  // other's fixture mid-test.
  const pkgName = "dirsql-testext-pkg-config";
  const pkgDir = join(tsNodeModules, pkgName);
  let tmp: string;

  // buildFixtureExtension shells out to a cargo build; the 10s default hook
  // timeout flakes under CI runner load, so allow 30s.
  beforeEach(() => {
    tmp = mkdtempSync(join(tmpdir(), "dirsql-cfg-ext-pkg-"));
    const so = buildFixtureExtension(join(tmp, "target"));
    // A real installed-package layout under the SDK's node_modules so
    // `require.resolve` finds it.
    mkdirSync(pkgDir, { recursive: true });
    writeFileSync(
      join(pkgDir, "package.json"),
      JSON.stringify({ name: pkgName, version: "0.0.0" }),
    );
    execFileSync("cp", [so, join(pkgDir, "testext.so")]);
  }, 30_000);

  afterEach(() => {
    rmSync(pkgDir, { recursive: true, force: true });
    rmSync(tmp, { recursive: true, force: true });
  });

  it("resolves, loads, and calls a config extension referenced by package name", async () => {
    // The config names the extension by bare package name; the index root is
    // set explicitly to the directory holding the config and its data.
    const root = join(tmp, "root");
    mkdirSync(root);
    const config = join(root, ".dirsql.toml");
    writeFileSync(
      config,
      `[[dirsql.extension]]\npath = "${pkgName}"\nentrypoint = "sqlite3_extension_init"\n`,
    );

    const db = new DirSQL({ root, config });
    await db.ready;
    expect(await db.query("SELECT dirsql_testext_answer() AS a")).toEqual([
      { a: 42 },
    ]);
  });
});
