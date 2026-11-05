// Binding-tier tests for what `onFile` is allowed to return.
//
// A row must be an object. A primitive fails the build naming the file, the
// same way a throwing hook does, rather than inserting an all-NULL row or, for
// `null`, aborting the process.
import { writeFileSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

describe("DirSQL onFile row shape", () => {
  let dir: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-row-shape-"));
    writeFileSync(join(dir, "marker.json"), "{}");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  const mkdb = (rows: unknown[]) =>
    new DirSQL({
      root: dir,
      tables: [
        {
          name: "t",
          ddl: "CREATE TABLE t (v)",
          glob: "*.json",
          onFile: () => rows as Record<string, unknown>[],
        },
      ],
    });

  const failsTheBuild = async (rows: unknown[]) => {
    const err = await mkdb(rows).ready.then(
      () => null,
      (e: Error) => e,
    );
    expect(err?.message).toContain("marker.json");
    return err?.message;
  };

  it("fails the build when a row is a number", async () => {
    expect(await failsTheBuild([1, 2])).toContain("array of objects");
  });

  it("fails the build when a row is a string", async () => {
    expect(await failsTheBuild(["a"])).toContain("array of objects");
  });

  it("fails the build when a row is null, rather than aborting the process", async () => {
    expect(await failsTheBuild([null])).toContain("array of objects");
  });

  it("fails the build when a row is a symbol, rather than aborting the process", async () => {
    expect(await failsTheBuild([Symbol("s")])).toContain("array of objects");
  });
});
