// SDK e2e: bind parameters through the built package. Nothing is mocked:
// real addon, real SQLite, real filesystem. A value shaped like SQL injection
// must bind as data and match exactly its own row.

import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const HOSTILE = `O'Brien said "hi"'; DROP TABLE t; --`;

describe("query bind parameters", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-e2e-query-params-"));
    await writeFile(join(dir, "hostile.json"), JSON.stringify({ v: HOSTILE }));
    await writeFile(join(dir, "plain.json"), JSON.stringify({ v: "O" }));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("binds a hostile string as data and returns exactly its row", async () => {
    const db = new DirSQL({ root: dir });
    const named = await db.query(
      "SELECT path, content ->> 'v' AS v FROM './*.json' WHERE content ->> 'v' = :v",
      { v: HOSTILE },
    );
    expect(named).toEqual([{ path: "hostile.json", v: HOSTILE }]);

    const positional = await db.query(
      "SELECT path FROM './*.json' WHERE content ->> 'v' = ?",
      [HOSTILE],
    );
    expect(positional).toEqual([{ path: "hostile.json" }]);

    const all = await db.query(
      "SELECT count(*) AS n FROM './*.json' WHERE content ->> 'v' != ?",
      [HOSTILE],
    );
    expect(all).toEqual([{ n: 1 }]);
  });
});
