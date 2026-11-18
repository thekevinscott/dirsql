// A relative `extensions` path is read when `new DirSQL` is called, so a chdir
// before the background build runs does not move it.
//
// The loadable is the repo's `tests/fixtures/testext` cdylib (registers
// `dirsql_testext_answer() -> 42`), built on the fly with cargo.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DirSQL } from "dirsql";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

const repoRoot = resolve(import.meta.dirname, "..", "..", "..", "..", "..");
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

describe("extensions at construction", () => {
  let tmp: string;
  let so: string;
  let original: string;

  // The cargo build outruns the 10s default hook timeout under CI load.
  beforeAll(() => {
    original = process.cwd();
    tmp = mkdtempSync(join(tmpdir(), "dirsql-ext-at-construction-"));
    so = buildFixtureExtension(join(tmp, "target"));
  }, 30_000);

  afterEach(() => {
    process.chdir(original);
  });

  afterAll(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it("resolves a relative path against the cwd of construction", async () => {
    const dir = mkdtempSync(join(tmp, "relative-"));
    mkdirSync(join(dir, "A", "ext"), { recursive: true });
    mkdirSync(join(dir, "B"));
    copyFileSync(so, join(dir, "A", "ext", "testext.so"));
    process.chdir(join(dir, "A"));
    const db = new DirSQL({
      root: join(dir, "A"),
      extensions: [
        { path: "ext/testext.so", entrypoint: "sqlite3_extension_init" },
      ],
    });
    process.chdir(join(dir, "B"));

    expect(await db.query("SELECT dirsql_testext_answer() AS a")).toEqual([
      { a: 42 },
    ]);
  });

  it("probes a bare-name shadow in the cwd of construction", async () => {
    const dir = mkdtempSync(join(tmp, "shadow-"));
    mkdirSync(join(dir, "A"));
    mkdirSync(join(dir, "B"));
    copyFileSync(so, join(dir, "A", "testext"));
    process.chdir(join(dir, "A"));
    const db = new DirSQL({
      root: join(dir, "A"),
      extensions: [{ path: "testext", entrypoint: "sqlite3_extension_init" }],
    });
    process.chdir(join(dir, "B"));

    expect(await db.query("SELECT dirsql_testext_answer() AS a")).toEqual([
      { a: 42 },
    ]);
  });
});
