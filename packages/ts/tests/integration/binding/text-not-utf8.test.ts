// Binding-tier tests for text SQLite holds as invalid UTF-8: the query
// returns it with replacement characters, as the CLI prints it, and the
// instance stays usable afterwards.
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-not-utf8-"));
  await writeFile(join(dir, "a.json"), "{}");
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

describe("DirSQL text that is not UTF-8", () => {
  it("returns replacement characters", async () => {
    const db = new DirSQL({ root: dir });
    const rows = await db.query("SELECT CAST(x'ff41' AS TEXT) AS v");
    expect(rows).toEqual([{ v: "�A" }]);
  });

  it("leaves the instance usable", async () => {
    const db = new DirSQL({ root: dir });
    await db.query("SELECT CAST(x'ff' AS TEXT) AS v FROM './a.json'");
    const rows = await db.query("SELECT basename FROM './a.json'");
    expect(rows).toEqual([{ basename: "a.json" }]);
  });
});
