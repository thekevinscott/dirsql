// Config-driven construction: `new DirSQL(configPath)`.
//
// Config-defined tables produce one row per matched file. Each row's columns
// come from filesystem facts: glob path captures and stat virtuals (`_path`,
// `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`). Content
// interpretation is intentionally out of scope; for that, register a
// programmatic Table with your own extract function.

import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

// Renamed from `writeFile` to avoid shadowing the `node:fs/promises` import;
// it ensures the parent directory exists before writing.
async function seedFile(path: string, content: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, content);
}

describe("new DirSQL(configPath)", () => {
  let dir: string;
  let configPath: string;

  beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "dirsql-fromconfig-"));
    configPath = join(dir, ".dirsql.toml");
  });

  afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
  });

  it("produces one row per matched file with stat virtuals", async () => {
    await seedFile(join(dir, "items", "a.csv"), "anything");
    await seedFile(join(dir, "items", "b.csv"), "anything");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (_path TEXT, _basename TEXT)"
glob = "items/*.csv"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const rows = await db.query(
      "SELECT _path, _basename FROM items ORDER BY _path",
    );
    expect(rows).toHaveLength(2);
    expect(rows[0]._path).toBe("items/a.csv");
    expect(rows[0]._basename).toBe("a.csv");
    expect(rows[1]._path).toBe("items/b.csv");
    expect(rows[1]._basename).toBe("b.csv");
  });

  it("injects glob path captures into rows", async () => {
    await seedFile(join(dir, "comments", "thread-1", "a.txt"), "x");
    await seedFile(join(dir, "comments", "thread-2", "a.txt"), "x");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE comments (thread_id TEXT, _basename TEXT)"
glob = "comments/{thread_id}/*.txt"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const rows = await db.query(
      "SELECT thread_id, _basename FROM comments ORDER BY thread_id",
    );
    expect(rows).toHaveLength(2);
    expect(rows[0].thread_id).toBe("thread-1");
    expect(rows[1].thread_id).toBe("thread-2");
  });

  it("exposes the full set of stat virtuals when declared in DDL", async () => {
    const body = "# title\nhello world\n";
    await seedFile(join(dir, "docs", "readme.md"), body);
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE files (_path TEXT, _basename TEXT, _dir TEXT, _ext TEXT, _size INTEGER, _mtime INTEGER)"
glob = "docs/*.md"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const rows = await db.query(
      "SELECT _path, _basename, _dir, _ext, _size, _mtime FROM files",
    );
    expect(rows).toHaveLength(1);
    const r = rows[0];
    expect(r._path).toBe("docs/readme.md");
    expect(r._basename).toBe("readme.md");
    expect(r._dir).toBe("docs");
    expect(r._ext).toBe("md");
    expect(r._size).toBe(body.length);
    expect(typeof r._mtime).toBe("number");
    expect(r._mtime as number).toBeGreaterThan(0);
  });

  it("respects ignore patterns from config", async () => {
    await seedFile(join(dir, "data", "good.json"), "{}");
    await seedFile(join(dir, "data", "node_modules", "bad.json"), "{}");
    await seedFile(
      configPath,
      `
[dirsql]
ignore = ["**/node_modules/**"]

[[table]]
ddl = "CREATE TABLE items (_path TEXT)"
glob = "data/**/*.json"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const rows = await db.query("SELECT _path FROM items");
    expect(rows).toHaveLength(1);
    expect(rows[0]._path).toBe("data/good.json");
  });

  it("loads multiple tables", async () => {
    await seedFile(join(dir, "posts", "hello.txt"), "x");
    await seedFile(join(dir, "authors", "alice.txt"), "x");
    await seedFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE posts (_basename TEXT)"
glob = "posts/*.txt"

[[table]]
ddl = "CREATE TABLE authors (_basename TEXT)"
glob = "authors/*.txt"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const posts = await db.query("SELECT _basename FROM posts");
    const authors = await db.query("SELECT _basename FROM authors");
    expect(posts).toHaveLength(1);
    expect(authors).toHaveLength(1);
    expect(posts[0]._basename).toBe("hello.txt");
    expect(authors[0]._basename).toBe("alice.txt");
  });

  it("rejects missing config files", async () => {
    const missing = join(dir, "nonexistent.toml");
    const db = new DirSQL(missing);
    await expect(db.ready).rejects.toThrow();
  });

  it("rejects invalid TOML", async () => {
    await seedFile(configPath, "this is not valid [[[");
    const db = new DirSQL(configPath);
    await expect(db.ready).rejects.toThrow();
  });

  it("rejects table entries missing ddl", async () => {
    await seedFile(
      configPath,
      `
[[table]]
glob = "*.json"
`,
    );
    const db = new DirSQL(configPath);
    await expect(db.ready).rejects.toThrow();
  });
});
