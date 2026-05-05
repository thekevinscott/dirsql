// Integration tests for config-driven construction: `new DirSQL(configPath)`.
// TS mirror of packages/python/tests/integration/test_from_config.py and
// packages/rust/tests/from_config.rs.
//
// Config-defined tables produce one row per matched file. Each row's columns
// come from filesystem facts: glob path captures and stat virtuals (`_path`,
// `_basename`, `_dir`, `_ext`, `_size`, `_mtime`, `_ctime`). Content
// interpretation is intentionally out of scope; for that, register a
// programmatic Table with your own extract function.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DirSQL } from "dirsql";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

function writeFile(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content);
}

describe("new DirSQL(configPath)", () => {
  let dir: string;
  let configPath: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "dirsql-fromconfig-"));
    configPath = join(dir, ".dirsql.toml");
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("produces one row per matched file with stat virtuals", async () => {
    writeFile(join(dir, "items", "a.csv"), "anything");
    writeFile(join(dir, "items", "b.csv"), "anything");
    writeFile(
      configPath,
      `
[[table]]
ddl = "CREATE TABLE items (_path TEXT, _basename TEXT)"
glob = "items/*.csv"
`,
    );

    const db = new DirSQL(configPath);
    await db.ready;
    const rows = await db.query("SELECT _path, _basename FROM items ORDER BY _path");
    expect(rows).toHaveLength(2);
    expect(rows[0]._path).toBe("items/a.csv");
    expect(rows[0]._basename).toBe("a.csv");
    expect(rows[1]._path).toBe("items/b.csv");
    expect(rows[1]._basename).toBe("b.csv");
  });

  it("injects glob path captures into rows", async () => {
    writeFile(join(dir, "comments", "thread-1", "a.txt"), "x");
    writeFile(join(dir, "comments", "thread-2", "a.txt"), "x");
    writeFile(
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
    writeFile(join(dir, "docs", "readme.md"), body);
    writeFile(
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
    writeFile(join(dir, "data", "good.json"), "{}");
    writeFile(join(dir, "data", "node_modules", "bad.json"), "{}");
    writeFile(
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
    writeFile(join(dir, "posts", "hello.txt"), "x");
    writeFile(join(dir, "authors", "alice.txt"), "x");
    writeFile(
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
    writeFile(configPath, "this is not valid [[[");
    const db = new DirSQL(configPath);
    await expect(db.ready).rejects.toThrow();
  });

  it("rejects table entries missing ddl", async () => {
    writeFile(
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
