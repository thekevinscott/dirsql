// Binding-tier tests: a folder a path-table's glob reaches that exists but
// cannot be listed rejects the query with an error naming it.
import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const lockable = process.platform !== "win32" && process.getuid?.() !== 0;

let dir: string;
let locked: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-unlistable-"));
  locked = join(dir, "data", "locked");
  await mkdir(join(dir, "data", "open"), { recursive: true });
  await mkdir(locked);
  await writeFile(join(dir, "data", "open", "a.json"), "{}");
  await writeFile(join(locked, "b.json"), "{}");
  await chmod(locked, 0o311);
});

afterEach(async () => {
  await chmod(locked, 0o755);
  await rm(dir, { recursive: true, force: true });
});

describe.runIf(lockable)("a path-table over an unlistable folder", () => {
  it("rejects with an error naming the folder", async () => {
    const db = new DirSQL({ root: dir });
    await expect(
      db.query("SELECT path FROM './data/**/*.json'"),
    ).rejects.toThrow(`cannot list folder '${locked}'`);
  });
});
