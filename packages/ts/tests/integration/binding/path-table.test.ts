import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// A table name SQLite does not know, but which looks like a path, resolves to
// a live glob scan of the index root. The logic lives in the Rust core, so the
// SDK inherits it -- these tests prove the inheritance crosses the napi
// boundary.
describe("path-tables (#627)", () => {
  let dir: string;

  const open = () =>
    new DirSQL({
      root: dir,
      tables: [
        {
          ddl: "CREATE TABLE rows_csv (path TEXT)",
          glob: "docs/*.csv",
          onFile: () => [{ path: "docs/c.csv" }],
        },
      ],
    });

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-627-"));
    await mkdir(join(dir, "docs"), { recursive: true });
    await writeFile(join(dir, "docs", "a.md"), "alpha");
    await writeFile(join(dir, "docs", "b.md"), "bravo body");
    await writeFile(join(dir, "docs", "c.csv"), "x,y");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("scans the index root for a bare './'", async () => {
    const rows = await open().query("SELECT path FROM './'");

    expect(rows.map((r) => r.path).sort()).toEqual([
      "docs/a.md",
      "docs/b.md",
      "docs/c.csv",
    ]);
  });

  it("scopes the scan to the glob", async () => {
    const rows = await open().query("SELECT basename, size FROM './docs/*.md'");

    expect(rows.map((r) => r.basename).sort()).toEqual(["a.md", "b.md"]);
    expect(rows.find((r) => r.basename === "b.md")?.size).toBe(10);
  });

  it("returns no rows when nothing matches", async () => {
    expect(await open().query("SELECT path FROM './docs/*.rst'")).toEqual([]);
  });

  it("joins a path-table against a named table", async () => {
    const rows = await open().query(
      "SELECT p.basename FROM './docs/*.csv' AS p JOIN rows_csv AS r ON r.path = p.path",
    );

    expect(rows.map((r) => r.basename)).toEqual(["c.csv"]);
  });

  it("still resolves a real table by name", async () => {
    expect(await open().query("SELECT path FROM rows_csv")).toEqual([
      { path: "docs/c.csv" },
    ]);
  });

  it("hints at the './' form for a bare glob", async () => {
    await expect(open().query("SELECT * FROM '**/*.md'")).rejects.toThrow(
      "did you mean './**/*.md'?",
    );
  });

  it("leaves a plain typo unchanged", async () => {
    const message = await open()
      .query("SELECT * FROM usrs")
      .then(
        () => "query unexpectedly succeeded",
        (err: Error) => err.message,
      );

    expect(message).toContain("no such table: usrs");
    expect(message).not.toContain("did you mean");
  });

  it("reads the filesystem live", async () => {
    const db = open();
    await db.query("SELECT path FROM './docs/*.md'");

    await writeFile(join(dir, "docs", "d.md"), "delta");

    const rows = await db.query("SELECT path FROM './docs/*.md'");
    expect(rows.map((r) => r.path)).toContain("docs/d.md");
  });

  it("excludes content from '*' but selects it by name", async () => {
    const starred = await open().query("SELECT * FROM './docs/*.md'");
    expect(starred[0]).not.toHaveProperty("content");

    const named = await open().query(
      "SELECT basename, content FROM './docs/*.md' WHERE basename = 'a.md'",
    );
    expect(named).toEqual([{ basename: "a.md", content: "alpha" }]);
  });

  it("yields null content for a non-UTF-8 file", async () => {
    await writeFile(
      join(dir, "docs", "logo.bin"),
      Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x80, 0x90]),
    );

    const rows = await open().query("SELECT content FROM './docs/*.bin'");
    expect(rows).toEqual([{ content: null }]);
  });
});
