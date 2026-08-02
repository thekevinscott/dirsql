// Binding-tier tests for value fidelity at the JS<->core boundary (#465):
// the numeric contract (out-of-range ints/BigInts error, never lossy) and
// BigInt->INTEGER, plus real onFile-error message propagation.
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

let dir: string;

beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), "dirsql-fidelity-"));
});

afterEach(async () => {
  await rm(dir, { recursive: true, force: true });
});

function mkdb(onFile: () => Array<Record<string, unknown>>): DirSQL {
  return new DirSQL({
    root: dir,
    tables: [{ ddl: "CREATE TABLE t (v)", glob: "*.json", onFile }],
  });
}

describe("DirSQL integer range (query results)", () => {
  beforeEach(async () => {
    await writeFile(join(dir, "marker.json"), "{}");
  });

  it("throws when an integer beyond MAX_SAFE_INTEGER is read back", async () => {
    // 2^53 + 1: a valid i64, but not a safe JS Number.
    const db = mkdb(() => [{ v: 9007199254740993n }]);
    await expect(db.query("SELECT v FROM t")).rejects.toThrow(/safe integer/);
  });

  it("round-trips an integer at MAX_SAFE_INTEGER", async () => {
    const db = mkdb(() => [{ v: 9007199254740991n }]);
    const rows = await db.query("SELECT v FROM t");
    expect(rows[0].v).toBe(9007199254740991);
  });
});

describe("DirSQL BigInt onFile", () => {
  beforeEach(async () => {
    await writeFile(join(dir, "marker.json"), "{}");
  });

  it("maps a small BigInt to INTEGER", async () => {
    const db = mkdb(() => [{ v: 42n }]);
    const rows = await db.query("SELECT v FROM t");
    expect(rows[0].v).toBe(42);
  });

  it("skips the file for a BigInt beyond i64 range", async () => {
    // Since dirsql#714 an unrepresentable value is the hook's mistake and
    // costs its own file; the build resolves with that file contributing
    // nothing. Which file was skipped is not reachable here yet (#715).
    const db = mkdb(() => [{ v: 2n ** 64n }]);
    await db.ready;
    expect(await db.query("SELECT v FROM t")).toEqual([]);
  });
});

describe("DirSQL onFile error message", () => {
  beforeEach(async () => {
    await writeFile(join(dir, "marker.json"), "{}");
  });

  it("skips the file whose onFile threw", async () => {
    // Since dirsql#714 a throwing hook no longer fails the scan. The message
    // is carried on the core's failure list, which this binding cannot reach
    // yet (#715) -- so all that is observable here is the absent row.
    const db = mkdb(() => {
      throw new Error("bad JSON in posts/a.json: boom");
    });
    await db.ready;
    expect(await db.query("SELECT v FROM t")).toEqual([]);
  });
});

describe("DirSQL Buffer -> BLOB", () => {
  const payload = Buffer.from([0x00, 0x01, 0x02, 0xff, 0xfe]);

  async function roundTrip(data: unknown): Promise<unknown> {
    await writeFile(join(dir, "marker.json"), "{}");
    const db = new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE blobs (name TEXT, data BLOB)",
          glob: "*.json",
          onFile: () => [{ name: "bin", data }],
        },
      ],
    });
    const rows = await db.query("SELECT * FROM blobs");
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("bin");
    return rows[0].data;
  }

  it("round-trips a Buffer through a BLOB column", async () => {
    const data = await roundTrip(payload);
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
  });

  it("maps a Uint8Array to a BLOB and returns it as a Buffer", async () => {
    const data = await roundTrip(new Uint8Array(payload));
    expect(Buffer.isBuffer(data)).toBe(true);
    expect(Buffer.compare(data as Buffer, payload)).toBe(0);
  });
});
