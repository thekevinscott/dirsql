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

  it("throws on a BigInt beyond i64 range", async () => {
    const db = mkdb(() => [{ v: 2n ** 64n }]);
    await expect(db.ready).rejects.toThrow(/i64|range|exceed|BigInt/i);
  });
});

describe("DirSQL onFile error message", () => {
  beforeEach(async () => {
    await writeFile(join(dir, "marker.json"), "{}");
  });

  it("propagates the thrown onFile error message", async () => {
    const db = mkdb(() => {
      throw new Error("bad JSON in posts/a.json: boom");
    });
    await expect(db.ready).rejects.toThrow(/bad JSON in posts\/a\.json: boom/);
  });
});
