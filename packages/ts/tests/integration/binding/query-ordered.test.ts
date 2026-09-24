import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("queryOrdered", () => {
  let dir: string;
  let db: DirSQL;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-query-ordered-"));
    await writeFile(
      join(dir, "users.json"),
      JSON.stringify([
        { name: "Alice", age: 30 },
        { name: "Bob", age: 25 },
      ]),
    );
    db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "users",
          ddl: "CREATE TABLE users (age INTEGER, name TEXT)",
          glob: "users.json",
          onFile: (filePath: string) =>
            JSON.parse(readFileSync(filePath, "utf8")),
        },
      ],
    });
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("reports the columns in the order the SELECT list names them", async () => {
    const result = await db.queryOrdered(
      "SELECT name, age, 1 AS a FROM users ORDER BY name",
    );
    expect(result.columns).toEqual(["name", "age", "a"]);
  });

  it("carries the same rows query returns", async () => {
    const sql = "SELECT name, age FROM users ORDER BY name";
    const result = await db.queryOrdered(sql);
    expect(result.rows).toEqual(await db.query(sql));
    expect(result.rows[0]).toEqual({ name: "Alice", age: 30 });
  });

  it("reports the columns of a query that matches no rows", async () => {
    const result = await db.queryOrdered(
      "SELECT name, age FROM users WHERE age > 100",
    );
    expect(result).toEqual({ columns: ["name", "age"], rows: [] });
  });
});
