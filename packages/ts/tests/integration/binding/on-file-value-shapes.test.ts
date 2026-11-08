// Binding-tier characterization of the JS value -> SQLite value mapping.
//
// value-fidelity.test.ts pins the numeric contract; this pins the rest of the
// match arms so a rewrite of the marshaling layer cannot quietly move one.
// Any other value type fails the build, naming the column and the type.
import { writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL onFile value shapes", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-value-shapes-"));
    writeFileSync(join(dir, "marker.json"), "{}");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const store = async (v: unknown): Promise<unknown> => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (v)",
          glob: "*.json",
          onFile: () => [{ v }] as Record<string, unknown>[],
        },
      ],
    });
    const rows = await db.query("SELECT v, typeof(v) AS ty FROM t");
    expect(rows).toHaveLength(1);
    return rows[0];
  };

  it("stores undefined as NULL", async () => {
    expect(await store(undefined)).toEqual({ v: null, ty: "null" });
  });

  it("stores null as NULL", async () => {
    expect(await store(null)).toEqual({ v: null, ty: "null" });
  });

  it("stores booleans as 1 and 0", async () => {
    expect(await store(true)).toEqual({ v: 1, ty: "integer" });
    expect(await store(false)).toEqual({ v: 0, ty: "integer" });
  });

  it("stores an integral double as INTEGER and a fractional one as REAL", async () => {
    expect(await store(2)).toEqual({ v: 2, ty: "integer" });
    expect(await store(1.5)).toEqual({ v: 1.5, ty: "real" });
  });

  it("stores a string as TEXT", async () => {
    expect(await store("hi")).toEqual({ v: "hi", ty: "text" });
  });

  it("stores a Uint8Array as a BLOB", async () => {
    const row = (await store(new Uint8Array([1, 2, 3]))) as {
      v: Buffer;
      ty: string;
    };
    expect(row.ty).toBe("blob");
    expect([...row.v]).toEqual([1, 2, 3]);
  });

  it("stores a Uint8ClampedArray as a BLOB", async () => {
    const row = (await store(new Uint8ClampedArray([4, 5]))) as {
      v: Buffer;
      ty: string;
    };
    expect(row.ty).toBe("blob");
    expect([...row.v]).toEqual([4, 5]);
  });

  it("stores an empty Uint8Array as an empty BLOB", async () => {
    const row = (await store(new Uint8Array([]))) as { v: Buffer; ty: string };
    expect(row.ty).toBe("blob");
    expect(row.v).toHaveLength(0);
  });

  it("honours a subarray's byte offset", async () => {
    const row = (await store(new Uint8Array([1, 2, 3, 4]).subarray(2))) as {
      v: Buffer;
      ty: string;
    };
    expect(row.ty).toBe("blob");
    expect([...row.v]).toEqual([3, 4]);
  });

  const refuse = async (v: unknown, typeName: string): Promise<void> => {
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (v)",
          glob: "*.json",
          onFile: () => [{ v }] as Record<string, unknown>[],
        },
      ],
    });
    await expect(db.ready).rejects.toThrow(
      `column 'v': unsupported value type ${typeName}`,
    );
  };

  it.each([
    ["Object", { a: 1 }],
    ["object", Object.create(null)],
    ["Array", [1, 2]],
    ["Date", new Date(0)],
    ["Map", new Map([["a", 1]])],
    ["Int16Array", new Int16Array([1, 2])],
    ["Int8Array", new Int8Array([1, 2])],
    ["function", () => 1],
    ["symbol", Symbol("s")],
  ])(
    "refuses a %s value rather than coercing it to a string",
    async (typeName, v) => {
      await refuse(v, typeName);
    },
  );
});
