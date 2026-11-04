import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const TRICKY = `it's "quoted"'; DROP TABLE notes; -- ünïcødé ✓`;

describe("query bind parameters", () => {
  let dir: string;

  const notes = () =>
    new DirSQL({
      root: dir,
      tables: [
        {
          name: "notes",
          ddl: "CREATE TABLE notes (name TEXT, body TEXT)",
          glob: "*.json",
          onFile: (filePath: string) => [
            {
              name: basename(filePath),
              body: JSON.parse(readFileSync(filePath, "utf8")).advice,
            },
          ],
        },
      ],
    });

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-query-params-"));
    await writeFile(join(dir, "a.json"), JSON.stringify({ advice: TRICKY }));
    await writeFile(join(dir, "b.json"), JSON.stringify({ advice: "plain" }));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("binds a named parameter for a path-table", async () => {
    const rows = await new DirSQL({ root: dir }).query(
      "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
      { advice: TRICKY },
    );
    expect(rows).toEqual([{ path: "a.json" }]);
  });

  it("binds an array for a declared table", async () => {
    const rows = await notes().query(
      "SELECT name, body FROM notes WHERE body = ?",
      [TRICKY],
    );
    expect(rows).toEqual([{ name: "a.json", body: TRICKY }]);
  });

  it("fills @name and $name placeholders from the same object", async () => {
    const rows = await notes().query(
      "SELECT name FROM notes WHERE body = @a OR body = $b ORDER BY name",
      { a: TRICKY, b: "plain" },
    );
    expect(rows).toEqual([{ name: "a.json" }, { name: "b.json" }]);
  });

  it("binds each native type as itself", async () => {
    const rows = await new DirSQL({ root: dir }).query(
      "SELECT typeof(?) AS i, typeof(?) AS r, typeof(?) AS t, typeof(?) AS b, " +
        "typeof(?) AS n, typeof(?) AS big, ? AS flag, ? AS blob",
      [
        7,
        1.5,
        "x",
        Buffer.from([0, 1]),
        null,
        9007199254740993n,
        true,
        new Uint8Array([2]),
      ],
    );
    expect(rows).toEqual([
      {
        i: "integer",
        r: "real",
        t: "text",
        b: "blob",
        n: "null",
        big: "integer",
        flag: 1,
        blob: Buffer.from([2]),
      },
    ]);
  });

  it("binds a bigint at the edge of the 64-bit range exactly", async () => {
    const rows = await new DirSQL({ root: dir }).query(
      "SELECT CAST(? AS TEXT) AS v",
      [-9223372036854775808n],
    );
    expect(rows).toEqual([{ v: "-9223372036854775808" }]);
  });

  it("binds through queryOrdered", async () => {
    const result = await notes().queryOrdered(
      "SELECT body, name FROM notes WHERE body = :body",
      { body: TRICKY },
    );
    expect(result).toEqual({
      columns: ["body", "name"],
      rows: [{ name: "a.json", body: TRICKY }],
    });
  });

  it("still runs a query without params", async () => {
    expect(await notes().query("SELECT count(*) AS n FROM notes")).toEqual([
      { n: 2 },
    ]);
  });

  it("rejects a value SQLite cannot hold with a TypeError", async () => {
    const db = new DirSQL({ root: dir });
    await expect(db.query("SELECT ?", [[1, 2]])).rejects.toThrow(TypeError);
    await expect(db.query("SELECT ?", [[1, 2]])).rejects.toThrow(
      "unsupported parameter type: object",
    );
  });

  it("rejects undefined as a value with a TypeError", async () => {
    await expect(
      new DirSQL({ root: dir }).query("SELECT :a", { a: undefined }),
    ).rejects.toThrow(
      expect.objectContaining({
        name: "TypeError",
        message: "unsupported parameter type: undefined",
      }),
    );
  });

  it("rejects params that are neither an array nor a plain object", async () => {
    const db = new DirSQL({ root: dir });
    for (const params of ["x", 1, new Map([["a", 1]]), Buffer.from("x")]) {
      await expect(
        db.query("SELECT ?", params as unknown as unknown[]),
      ).rejects.toThrow(
        expect.objectContaining({
          name: "TypeError",
          message: expect.stringMatching(
            /^params must be an array or a plain object, not /,
          ),
        }),
      );
    }
  });

  it("rejects a bigint outside the 64-bit range with a RangeError", async () => {
    await expect(
      new DirSQL({ root: dir }).query("SELECT ?", [2n ** 63n]),
    ).rejects.toThrow(
      expect.objectContaining({
        name: "RangeError",
        message:
          "BigInt 9223372036854775808 exceeds the 64-bit signed range dirsql can store",
      }),
    );
  });

  it("names a wrong parameter count", async () => {
    await expect(
      new DirSQL({ root: dir }).query("SELECT ?, ?", [1]),
    ).rejects.toThrow(
      "the query has 2 parameter(s), but 1 value(s) were given",
    );
  });

  it("names a missing named parameter", async () => {
    await expect(
      new DirSQL({ root: dir }).query(
        "SELECT path FROM './*.json' WHERE content ->> 'advice' = :advice",
        {},
      ),
    ).rejects.toThrow("the named parameter :advice has no value");
  });

  it("names a key no placeholder uses", async () => {
    await expect(
      new DirSQL({ root: dir }).query("SELECT :a", { a: 1, z: 2 }),
    ).rejects.toThrow('the parameter "z" is not a placeholder in the query');
  });

  it("refuses an array for named placeholders", async () => {
    await expect(
      new DirSQL({ root: dir }).query("SELECT :a", [1]),
    ).rejects.toThrow(
      "the query uses the named parameter :a; pass a mapping, not a sequence",
    );
  });
});
