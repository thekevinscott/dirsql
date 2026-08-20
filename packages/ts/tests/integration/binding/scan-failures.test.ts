// Binding-tier tests for the scan's record of files it skipped (#715).
//
// Since #714 a file whose `onFile` hook throws, or whose row the table
// rejects, is skipped rather than failing the scan. The CLI reports those
// skips on stderr and exits 23; a TypeScript caller had no equivalent, so an
// incomplete index was indistinguishable from a complete one -- the
// regression this closes.
import { writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL scanFailures", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-scan-failures-"));
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const write = (...names: string[]) => {
    for (const name of names) {
      writeFileSync(join(dir, name), "{}");
    }
  };

  const mkdb = (
    onFile: (filePath: string) => unknown[],
    extra: Record<string, unknown> = {},
  ) =>
    new DirSQL({
      root: dir,
      tables: [
        {
          name: "items",
          ddl: "CREATE TABLE items (name TEXT)",
          glob: "*.json",
          onFile,
          ...extra,
        },
      ],
    });

  it("is empty after a clean scan", async () => {
    write("a.json");
    const db = mkdb(() => [{ name: "ok" }]);
    await db.ready;
    expect(await db.scanFailures()).toEqual([]);
  });

  it("names each skipped file", async () => {
    write("good.json", "bad.json");
    const db = mkdb((filePath) => {
      if (filePath.endsWith("bad.json")) {
        throw new Error("boom");
      }
      return [{ name: "ok" }];
    });
    await db.ready;

    const failures = await db.scanFailures();
    expect(failures).toHaveLength(1);
    expect(basename(failures[0].path)).toBe("bad.json");
    // The row that did land is untouched: this reports, it does not gate.
    expect(await db.query("SELECT name FROM items")).toEqual([{ name: "ok" }]);
  });

  it("carries the hook's own message", async () => {
    write("a.json");
    const db = mkdb(() => {
      throw new Error("boom-xyzzy");
    });
    await db.ready;

    const [failure] = await db.scanFailures();
    expect(failure.message).toContain("boom-xyzzy");
  });

  it("reports a row the table rejected", async () => {
    // Not just thrown hooks: a strict-mode violation is the same kind of
    // per-file failure, and the message still names the offending column.
    write("a.json");
    const db = mkdb(() => [{ nope: 1 }], { strict: true });
    await db.ready;

    const [failure] = await db.scanFailures();
    expect(basename(failure.path)).toBe("a.json");
    expect(failure.message).toContain("nope");
  });

  it("reports every skipped file, not only the first", async () => {
    write("a.json", "b.json", "c.json");
    const db = mkdb(() => {
      throw new Error("boom");
    });
    await db.ready;

    const names = (await db.scanFailures()).map((f) => basename(f.path)).sort();
    expect(names).toEqual(["a.json", "b.json", "c.json"]);
  });
});
