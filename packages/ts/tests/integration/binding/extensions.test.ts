// Asserts via the failure path — a nonexistent library surfaces a "failed to
// load extension" error from the core — because that proves the `extensions`
// option actually reached the loader without requiring a real
// platform-specific shared library in CI.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL extensions option (#230)", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-extensions-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("rejects ready when a configured extension cannot be loaded", async () => {
    const db = new DirSQL({
      root: dir,
      extensions: [{ path: join(dir, "nonexistent-dirsql.so") }],
    });
    await expect(db.ready).rejects.toThrow(/failed to load extension/);
  });

  // The missing library still fails with the entrypoint set, proving the
  // entry reached the core verbatim.
  it("carries an optional entrypoint into the load call", async () => {
    const db = new DirSQL({
      root: dir,
      extensions: [
        {
          path: join(dir, "nonexistent-dirsql-y.so"),
          entrypoint: "sqlite3_y_init",
        },
      ],
    });
    await expect(db.ready).rejects.toThrow(/failed to load extension/);
  });

  it("does not reject ready when no extensions are configured", async () => {
    const db = new DirSQL({ root: dir });
    await expect(db.ready).resolves.toBeUndefined();
  });
});

// A `[[dirsql.extension]]` entry in a `.dirsql.toml` config is loaded by the
// core's config loader — a missing library fails the build, confirming the
// config path delegates extension loading to the core.
describe("DirSQL config-file extensions", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-cfg-extensions-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("rejects ready when a config-file extension cannot be loaded", async () => {
    const cfgPath = join(dir, ".dirsql.toml");
    await writeFile(
      cfgPath,
      '[[dirsql.extension]]\npath = "missing-extension.so"\n',
    );

    const db = new DirSQL(cfgPath);
    await expect(db.ready).rejects.toThrow(/failed to load extension/);
  });
});
