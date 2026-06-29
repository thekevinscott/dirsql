// Integration test for the `extensions` constructor option (#230).
//
// Exercises the SDK public API end to end: the `extensions` option must be
// marshaled through the napi binding into the Rust core's builder, which
// loads each extension onto the connection at startup. We assert via the
// failure path — a nonexistent library surfaces a "failed to load
// extension" error from the core (mirroring the Rust core's
// `missing_extension_build_fails_with_extension_error`) — because that
// proves the option actually reached the loader without requiring a real
// platform-specific shared library in CI.

import { mkdtemp, rm } from "node:fs/promises";
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

  it("does not reject ready when no extensions are configured", async () => {
    const db = new DirSQL({ root: dir });
    await expect(db.ready).resolves.toBeUndefined();
  });
});
