// Binding-tier tests for text SQLite holds as invalid UTF-8: the query
// rejects with an error naming the column, and the instance stays usable
// afterwards.
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const MESSAGE = 'column "v" holds text that is not valid UTF-8';

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-not-utf8-"));
  await writeFile(join(dir, "a.json"), "{}");
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

describe("DirSQL text that is not UTF-8", () => {
  it("rejects with an error naming the column", async () => {
    const db = new DirSQL({ root: dir });
    await expect(db.query("SELECT CAST(x'ff41' AS TEXT) AS v")).rejects.toThrow(
      MESSAGE,
    );
  });

  it("leaves the instance usable", async () => {
    const db = new DirSQL({ root: dir });
    await expect(
      db.query("SELECT CAST(x'ff' AS TEXT) AS v FROM './a.json'"),
    ).rejects.toThrow(MESSAGE);
    const rows = await db.query("SELECT basename FROM './a.json'");
    expect(rows).toEqual([{ basename: "a.json" }]);
  });
});
